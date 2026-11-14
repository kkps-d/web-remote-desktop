using Agent;

namespace Agent.Tests;

public class H264CodecTests
{
    // SDP lines end in CRLF, which leaves a trailing \r on each matched fmtp value.
    static string Offer(params string[] lines) =>
        string.Join("\r\n", ["v=0", "m=video 9 UDP/TLS/RTP/SAVPF 96 102 104 106 108", .. lines, ""]);

    [Fact]
    public void PicksTheFirstConstrainedBaselineCodecWithPacketizationMode1()
    {
        // Chrome lists baseline and packetization-mode=0 variants before the one we can use.
        var offer = Offer(
            "a=rtpmap:96 VP8/90000",
            "a=rtpmap:102 H264/90000",
            "a=fmtp:102 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f",
            "a=rtpmap:104 H264/90000",
            "a=fmtp:104 level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f",
            "a=rtpmap:106 H264/90000",
            "a=fmtp:106 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f",
            "a=rtpmap:108 H264/90000",
            "a=fmtp:108 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e034");

        Assert.Equal(new StreamSession.H264Codec(106, "42e01f"), StreamSession.FindH264Codec(offer));
    }

    [Fact]
    public void AcceptsFirefoxParameterOrder()
    {
        var offer = Offer(
            "a=rtpmap:126 H264/90000",
            "a=fmtp:126 profile-level-id=42e01f;level-asymmetry-allowed=1;packetization-mode=1",
            "a=rtpmap:97 H264/90000",
            "a=fmtp:97 profile-level-id=42e01f;level-asymmetry-allowed=1");

        Assert.Equal(new StreamSession.H264Codec(126, "42e01f"), StreamSession.FindH264Codec(offer));
    }

    [Theory]
    [InlineData("a=rtpmap:96 VP8/90000")]
    [InlineData("a=rtpmap:102 H264/90000", "a=fmtp:102 packetization-mode=1;profile-level-id=42001f")]
    [InlineData("a=rtpmap:102 H264/90000", "a=fmtp:102 packetization-mode=0;profile-level-id=42e01f")]
    [InlineData("a=rtpmap:102 H264/90000", "a=fmtp:102 packetization-mode=1;profile-level-id=640c1f")]
    [InlineData("a=rtpmap:102 H264/90000")]
    public void RejectsOffersWithoutAUsableCodec(params string[] lines) =>
        Assert.Throws<InvalidOfferException>(() => StreamSession.FindH264Codec(Offer(lines)));

    [Fact]
    public void DoesNotReadAnotherPayloadTypesParameters()
    {
        // 10 has no fmtp line; 102's must not be taken for it.
        var offer = Offer(
            "a=rtpmap:10 H264/90000",
            "a=rtpmap:102 VP8/90000",
            "a=fmtp:102 packetization-mode=1;profile-level-id=42e01f");

        Assert.Throws<InvalidOfferException>(() => StreamSession.FindH264Codec(offer));
    }
}
