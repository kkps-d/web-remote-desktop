namespace Agent;

/// <summary>Holds the single active viewer session. A new viewer replaces the current one.</summary>
sealed class ViewerSessions(ILogger<ViewerSessions> logger) : IDisposable
{
    // Serializes setup so only one pipeline captures the desktop at a time. Setup blocks for
    // seconds, so it has its own lock and stopping a session never waits for it.
    readonly Lock setupGate = new();
    readonly Lock gate = new();
    StreamSession? current;

    public StreamSession Start(string offerSdp)
    {
        // Reject an unusable offer before it disconnects the current viewer.
        var codec = StreamSession.FindH264Codec(offerSdp);

        lock (setupGate)
        {
            lock (gate)
            {
                if (current != null)
                {
                    logger.LogInformation("Replacing session {Id} with a new viewer", current.Id);
                    StopCurrent();
                }
            }

            var session = StreamSession.Start(offerSdp, codec, logger);
            session.Ended += ended => Stop(ended.Id);
            lock (gate)
            {
                // Stop ignores a session that isn't current yet, so catch one that already ended.
                if (session.EndReason is { } reason)
                {
                    session.Dispose();
                    throw new InvalidOperationException(reason);
                }
                current = session;
            }
            logger.LogInformation("Session {Id} started", session.Id);
            return session;
        }
    }

    /// <summary>Stops the session if it is still the current one. Returns false if it isn't.</summary>
    public bool Stop(string id)
    {
        lock (gate)
        {
            if (current?.Id != id) return false;
            logger.LogInformation("Session {Id} stopped", id);
            StopCurrent();
            return true;
        }
    }

    /// <summary>Returns the session's stream info, or null if it isn't the current session.</summary>
    public StreamInfo? GetInfo(string id)
    {
        lock (gate) return current?.Id == id ? Info(current) : null;
    }

    /// <summary>Changes the session's stream settings. Returns null if it isn't the current session.</summary>
    public StreamInfo? Update(string id, StreamSettingsUpdate update)
    {
        // Holding the gate keeps the session from being disposed mid-update.
        lock (gate)
        {
            if (current?.Id != id) return null;
            current.Update(update);
            return Info(current);
        }
    }

    static StreamInfo Info(StreamSession session) => new(session.SourceResolution, session.Settings);

    void StopCurrent()
    {
        current?.Dispose();
        current = null;
    }

    public void Dispose()
    {
        lock (setupGate)
        lock (gate)
            StopCurrent();
    }
}
