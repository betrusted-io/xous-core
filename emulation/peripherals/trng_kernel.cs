//
// Copyright (c) 2010-2019 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class BetrustedRNGKernel : BasicDoubleWordPeripheral, IKnownSize
    {
        public BetrustedRNGKernel(Machine machine) : base(machine)
        {
            this.IRQ = new GPIO();
            DefineRegisters();
        }

        public long Size { get { return 0x1000; } }

        private void DefineRegisters()
        {
            Registers.STATUS.Define(this) // RDY is set on reset
                .WithValueField(0, 32, FieldMode.Read, valueProviderCallback: _ =>
                {
                    return 3;
                }, name: "STATUS");

            Registers.DATA.Define(this)
                .WithValueField(0, 32, FieldMode.Read, valueProviderCallback: _ =>
                {
                    return (uint)rng.Next();
                }, name: "DATA");
        }

        private readonly PseudorandomNumberGenerator rng = EmulationManager.Instance.CurrentEmulation.RandomGenerator;
        public GPIO IRQ { get; private set; }

        private enum Registers
        {
            STATUS = 0x0,
            DATA = 0x4,
            EV_STATUS = 0x8,
            EV_PENDING = 0x0c,
            EV_ENABLE = 0x10,
        }
    }
}
