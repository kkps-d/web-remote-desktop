using System.Collections;
using System.Reflection;
using System.Runtime.InteropServices;

namespace Agent;

/// <summary>Locates the GStreamer install and initializes the C# bindings against it.</summary>
static class GStreamerRuntime
{
    public static void Initialize()
    {
        // The official installer (per-user, MSVC build) doesn't add itself to PATH.
        var root = Environment.GetEnvironmentVariable("GSTREAMER_1_0_ROOT_MSVC_X86_64")
            ?? Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                @"Programs\gstreamer\1.0\msvc_x86_64");
        var bin = Path.Combine(root, "bin");
        if (!File.Exists(Path.Combine(bin, "gstreamer-1.0-0.dll")))
            throw new InvalidOperationException($"GStreamer not found in {bin}. Install it with the \"Complete\" option.");

        Environment.SetEnvironmentVariable("PATH", bin + ";" + Environment.GetEnvironmentVariable("PATH"));
        UseMsvcGLibNames(bin);
        Gst.Application.Init();
    }

    // GLibSharp only knows MinGW DLL names (libgobject-2.0-0.dll), but the official MSVC
    // GStreamer build ships gobject-2.0-0.dll. Loading renamed copies would create a second
    // GLib instance, so instead pre-fill GLibSharp's private handle cache with the real DLLs.
    static void UseMsvcGLibNames(string bin)
    {
        var glibSharp = typeof(GLib.Object).Assembly;
        var cache = (IDictionary)glibSharp.GetType("GLibrary")!
            .GetField("_libraries", BindingFlags.NonPublic | BindingFlags.Static)!
            .GetValue(null)!;
        var libraryEnum = glibSharp.GetType("Library")!;

        foreach (var (library, dll) in new[] { ("GLib", "glib-2.0-0.dll"), ("GObject", "gobject-2.0-0.dll"), ("Gio", "gio-2.0-0.dll") })
        {
            cache[Enum.Parse(libraryEnum, library)] = NativeLibrary.Load(Path.Combine(bin, dll));
        }
    }
}
