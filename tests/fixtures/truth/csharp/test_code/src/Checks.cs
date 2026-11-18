using NUnit.Framework;
using Xunit;

namespace App
{
    [TestFixture]
    public class NUnitChecks
    {
        [SetUp]
        public void Prepare()
        {
        }

        [Test]
        public void GivesOne()
        {
            Check(1);
        }

        private void Check(int value)
        {
            Assert.That(new Service().Production(), Is.EqualTo(value));
        }
    }

    public class XunitChecks
    {
        [Fact]
        public void IsOne()
        {
            Build();
        }

        [Theory, InlineData(1)]
        public void AcceptsEachValue(int value)
        {
            Build();
        }

        private Service Build()
        {
            return new Service();
        }
    }
}
