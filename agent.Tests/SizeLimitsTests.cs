using Agent;

namespace Agent.Tests;

public class SizeLimitsTests
{
    static readonly SizeLimits Nvenc = new(145, 4096, 49, 4096);

    [Theory]
    [InlineData(145, 49, true)]
    [InlineData(4096, 4096, true)]
    [InlineData(144, 1080, false)]
    [InlineData(1920, 48, false)]
    [InlineData(4097, 1080, false)]
    [InlineData(1920, 4097, false)]
    public void FitsIncludesBothEnds(int width, int height, bool fits) =>
        Assert.Equal(fits, Nvenc.Fits(new Resolution(width, height)));

    [Fact]
    public void ShrinksA5KDisplayToTheLimitKeepingItsAspectRatio() =>
        Assert.Equal(new Resolution(4096, 2304), Nvenc.ShrinkToFit(new Resolution(5120, 2880)));

    [Fact]
    public void ShrinksToEvenDimensions()
    {
        var size = Nvenc.ShrinkToFit(new Resolution(5000, 3001));

        Assert.True(Nvenc.Fits(size));
        Assert.Equal(0, size.Width % 2);
        Assert.Equal(0, size.Height % 2);
        Assert.Equal(5000.0 / 3001, (double)size.Width / size.Height, 2);
    }

    [Fact]
    public void LeavesASizeThatFitsAlone() =>
        Assert.Equal(new Resolution(1920, 1080), Nvenc.ShrinkToFit(new Resolution(1920, 1080)));
}
