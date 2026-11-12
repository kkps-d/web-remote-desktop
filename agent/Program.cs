using Agent;

var builder = WebApplication.CreateBuilder(args);

// Port comes from appsettings.json or the command line, e.g. `dotnet run -- --Port 9000`.
var port = builder.Configuration.GetValue("Port", 8080);
builder.WebHost.ConfigureKestrel(kestrel => kestrel.ListenAnyIP(port));

// The web client is hosted on a different origin, and needs the Location header to end its session.
builder.Services.AddCors(cors => cors.AddDefaultPolicy(policy =>
    policy.AllowAnyOrigin().AllowAnyHeader().AllowAnyMethod().WithExposedHeaders("Location")));
builder.Services.AddSingleton<ViewerSessions>();

GStreamerRuntime.Initialize();

var app = builder.Build();
app.UseCors();

// WHEP-style signaling: POST an SDP offer, get the SDP answer and a session URL back.
app.MapPost("/offer", async (HttpRequest request, HttpResponse response, ViewerSessions sessions, ILogger<Program> logger) =>
{
    if (request.ContentType?.StartsWith("application/sdp") != true)
        return Results.Text("Expected Content-Type: application/sdp", statusCode: 415);

    var offer = await new StreamReader(request.Body).ReadToEndAsync();
    try
    {
        // Session setup blocks on GStreamer for up to a few seconds; keep it off the request thread.
        var session = await Task.Run(() => sessions.Start(offer));
        response.Headers.Location = $"/sessions/{session.Id}";
        return Results.Text(session.AnswerSdp, "application/sdp", statusCode: 201);
    }
    catch (InvalidOfferException ex)
    {
        return Results.Text(ex.Message, statusCode: 400);
    }
    catch (Exception ex)
    {
        logger.LogError(ex, "Failed to start a session");
        return Results.Text($"Could not start the stream: {ex.Message}", statusCode: 500);
    }
});

app.MapDelete("/sessions/{id}", (string id, ViewerSessions sessions) =>
    sessions.Stop(id) ? Results.NoContent() : Results.NotFound());

app.Run();
