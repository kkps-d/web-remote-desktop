using Agent;

namespace Agent.Tests;

public class OpusPayloadTypeTests
{
    static string Offer(params string[] lines) =>
        string.Join("\r\n", ["v=0", "m=video 9 UDP/TLS/RTP/SAVPF 106", "a=rtpmap:106 H264/90000", .. lines, ""]);

    [Fact]
    public void FindsChromesOpusPayloadType()
    {
        var offer = Offer(
            "m=audio 9 UDP/TLS/RTP/SAVPF 111 63 9",
            "a=rtpmap:111 opus/48000/2",
            "a=fmtp:111 minptime=10;useinbandfec=1",
            "a=rtpmap:63 red/48000/2",
            "a=rtpmap:9 G722/8000");

        Assert.Equal(111, StreamSession.FindOpusPayloadType(offer));
    }

    [Fact]
    public void MatchesTheEncodingNameInAnyCase() =>
        Assert.Equal(109, StreamSession.FindOpusPayloadType(Offer("m=audio 9 UDP/TLS/RTP/SAVPF 109", "a=rtpmap:109 OPUS/48000/2")));

    [Theory]
    [InlineData]
    [InlineData("m=audio 9 UDP/TLS/RTP/SAVPF 9", "a=rtpmap:9 G722/8000")]
    public void ReturnsNullWithoutOpus(params string[] lines) =>
        Assert.Null(StreamSession.FindOpusPayloadType(Offer(lines)));
}
