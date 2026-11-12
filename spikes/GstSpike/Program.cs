// Spike: stream the desktop to a browser with webrtcbin, driven from C#.
//   http://localhost:8080/offer  - signaling endpoint (POST SDP offer, get SDP answer back)
//   http://localhost:8081/       - bare test page, on a different origin to exercise CORS
using System.Collections;
using System.Net;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using Gst;
using Gst.Sdp;
using Gst.WebRTC;

// webrtcbin matches our stream to the browser's offer by intersecting RTP caps exactly, so the
// payload type and profile-level-id must be the browser's. The real stream needs a higher H.264
// level for desktop resolutions; browsers accept that (level-asymmetry-allowed=1), so capssetter
// just relabels the caps.
static string PipelineDescription(H264Codec codec) =>
    "d3d11screencapturesrc show-cursor=true ! d3d11convert ! " +
    "video/x-raw(memory:D3D11Memory),format=NV12 ! " +
    "mfh264enc low-latency=true bitrate=8000 gop-size=60 ! h264parse ! " +
    "video/x-h264,profile=constrained-baseline ! " +
    $"rtph264pay pt={codec.PayloadType} config-interval=-1 aggregate-mode=zero-latency ! " +
    $"capssetter caps=\"application/x-rtp,profile-level-id=(string){codec.ProfileLevelId}\" ! " +
    "webrtcbin name=webrtc bundle-policy=max-bundle";

// Finds the browser's constrained-baseline, packetization-mode=1 H.264 codec in the offer.
static H264Codec FindH264Codec(string offerSdp)
{
    foreach (Match rtpmap in Regex.Matches(offerSdp, @"a=rtpmap:(\d+) H264/90000"))
    {
        var payloadType = rtpmap.Groups[1].Value;
        var fmtp = Regex.Match(offerSdp, $@"a=fmtp:{payloadType} (.*)").Groups[1].Value;
        var profileLevelId = Regex.Match(fmtp, "profile-level-id=(42e0[0-9a-f]{2})").Groups[1].Value;
        if (fmtp.Contains("packetization-mode=1") && profileLevelId != "")
            return new H264Codec(int.Parse(payloadType), profileLevelId);
    }
    throw new InvalidOperationException("Offer has no constrained-baseline H.264 codec");
}

// The GStreamer installer doesn't add itself to PATH, so point the native DLL loader at it.
var gstBin = Path.Combine(
    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
    @"Programs\gstreamer\1.0\msvc_x86_64\bin");
Environment.SetEnvironmentVariable("PATH", gstBin + ";" + Environment.GetEnvironmentVariable("PATH"));

UseMsvcGLibNames(gstBin);
Application.Init();
Console.WriteLine($"GStreamer {Application.VersionString()}");

var testPage = File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "test.html"));
Pipeline? current = null;

var listener = new HttpListener();
listener.Prefixes.Add("http://localhost:8080/");
listener.Prefixes.Add("http://localhost:8081/");
listener.Start();
Console.WriteLine("Signaling on http://localhost:8080/offer, test page on http://localhost:8081/");

while (true)
{
    var context = listener.GetContext();
    var request = context.Request;
    var response = context.Response;
    try
    {
        if (request.Url!.Port == 8081)
        {
            Respond(response, 200, "text/html", testPage);
            continue;
        }

        response.AddHeader("Access-Control-Allow-Origin", "*");
        if (request.HttpMethod == "OPTIONS")
        {
            response.AddHeader("Access-Control-Allow-Methods", "POST");
            response.AddHeader("Access-Control-Allow-Headers", "Content-Type");
            Respond(response, 204, null, null);
        }
        else if (request.HttpMethod == "POST" && request.Url.AbsolutePath == "/offer")
        {
            using var reader = new StreamReader(request.InputStream);
            var offer = reader.ReadToEnd();
            Respond(response, 201, "application/sdp", Answer(offer));
        }
        else
        {
            Respond(response, 404, null, null);
        }
    }
    catch (Exception ex)
    {
        Console.WriteLine($"Request failed: {ex}");
        Respond(response, 500, "text/plain", ex.Message);
    }
}

// Starts a fresh pipeline for the offer and returns webrtcbin's answer, with all ICE candidates included.
string Answer(string offerSdp)
{
    // One viewer at a time for the spike: drop any previous session.
    current?.SetState(State.Null);
    current = (Pipeline)Parse.Launch(PipelineDescription(FindH264Codec(offerSdp)));
    var webrtc = current.GetByName("webrtc");
    LogBusMessages(current);
    current.SetState(State.Playing);

    // webrtcbin matches our video pad to the browser's m-line by caps. Until the first frame
    // arrives the pad has none, and webrtcbin answers with an unrelated receive-only transceiver.
    var videoPad = webrtc.GetStaticPad("sink_0");
    var capsDeadline = System.DateTime.UtcNow.AddSeconds(5);
    while (videoPad.CurrentCaps == null)
    {
        if (System.DateTime.UtcNow > capsDeadline) throw new TimeoutException("No video caps from the capture pipeline");
        Thread.Sleep(10);
    }
    Console.WriteLine($"Video caps: {videoPad.CurrentCaps}");

    SDPMessage.NewFromText(offerSdp, out var sdp);
    EmitAndWait(webrtc, "set-remote-description", WebRTCSessionDescription.New(WebRTCSDPType.Offer, sdp));

    // The bindings can't marshal a null argument, so pass an empty options structure.
    var reply = EmitAndWait(webrtc, "create-answer", new Structure("options"));
    var answer = (WebRTCSessionDescription)reply!.GetValue("answer");
    EmitAndWait(webrtc, "set-local-description", answer);

    // Non-trickle ICE: wait until every local candidate is in the local description.
    var deadline = System.DateTime.UtcNow.AddSeconds(10);
    while (Convert.ToInt32(webrtc["ice-gathering-state"]) != (int)WebRTCICEGatheringState.Complete)
    {
        if (System.DateTime.UtcNow > deadline) throw new TimeoutException("ICE gathering did not complete");
        Thread.Sleep(20);
    }

    Console.WriteLine("Answered offer");
    return Native.GetLocalDescriptionText(webrtc);
}

// webrtcbin's action signals take a GstPromise as their last argument and resolve it when done.
static Structure? EmitAndWait(Element webrtc, string signal, object? argument)
{
    var promise = new Promise();
    webrtc.Emit(signal, argument, promise);
    if (promise.Wait() != PromiseResult.Replied) throw new InvalidOperationException($"{signal} did not reply");
    return promise.RetrieveReply();
}

static void LogBusMessages(Pipeline pipeline)
{
    var bus = pipeline.Bus;
    new Thread(() =>
    {
        while (true)
        {
            var message = bus.TimedPopFiltered(Gst.Constants.CLOCK_TIME_NONE, MessageType.Error | MessageType.Warning | MessageType.Eos);
            if (message.Type == MessageType.Eos) return;
            // The message structure holds both the error text and the debug details.
            Console.WriteLine($"{message.Type} from {message.Src?.Name}: {message.Structure}");
            if (message.Type == MessageType.Error) return;
        }
    }) { IsBackground = true }.Start();
}

static void Respond(HttpListenerResponse response, int status, string? contentType, string? body)
{
    response.StatusCode = status;
    if (contentType != null) response.ContentType = contentType;
    if (body != null)
    {
        var bytes = Encoding.UTF8.GetBytes(body);
        response.OutputStream.Write(bytes);
    }
    response.Close();
}

// GLibSharp only knows MinGW DLL names (libgobject-2.0-0.dll), but the official MSVC
// GStreamer build ships gobject-2.0-0.dll. Loading renamed copies would create a second
// GLib instance, so instead pre-fill GLibSharp's private handle cache with the real DLLs.
static void UseMsvcGLibNames(string gstBin)
{
    var glibSharp = typeof(GLib.Object).Assembly;
    var cache = (IDictionary)glibSharp.GetType("GLibrary")!
        .GetField("_libraries", BindingFlags.NonPublic | BindingFlags.Static)!
        .GetValue(null)!;
    var libraryEnum = glibSharp.GetType("Library")!;

    foreach (var (library, dll) in new[] { ("GLib", "glib-2.0-0.dll"), ("GObject", "gobject-2.0-0.dll"), ("Gio", "gio-2.0-0.dll") })
    {
        cache[Enum.Parse(libraryEnum, library)] = NativeLibrary.Load(Path.Combine(gstBin, dll));
    }
}

// Direct native calls for things the bindings get wrong.
static class Native
{
    [DllImport("gobject-2.0-0.dll")]
    static extern void g_object_get(IntPtr obj, string property, out IntPtr value, IntPtr terminator);

    [DllImport("gstsdp-1.0-0.dll")]
    static extern IntPtr gst_sdp_message_as_text(IntPtr sdp);

    [DllImport("gstwebrtc-1.0-0.dll")]
    static extern void gst_webrtc_session_description_free(IntPtr description);

    [DllImport("glib-2.0-0.dll")]
    static extern void g_free(IntPtr memory);

    // Reading "local-description" through the bindings frees the boxed copy too early and the
    // SDP pointer dangles. g_object_get hands us our own copy, which we free when done.
    public static string GetLocalDescriptionText(Element webrtc)
    {
        g_object_get(webrtc.Handle, "local-description", out var description, IntPtr.Zero);
        try
        {
            // struct GstWebRTCSessionDescription { GstWebRTCSDPType type; GstSDPMessage *sdp; }
            var sdp = Marshal.ReadIntPtr(description, IntPtr.Size);
            var text = gst_sdp_message_as_text(sdp);
            try { return Marshal.PtrToStringUTF8(text)!; }
            finally { g_free(text); }
        }
        finally
        {
            gst_webrtc_session_description_free(description);
        }
    }
}

record H264Codec(int PayloadType, string ProfileLevelId);
