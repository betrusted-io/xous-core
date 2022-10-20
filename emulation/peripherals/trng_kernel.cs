//
// Copyright (c) 2010-2019 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;

namespace Antmicro.Renode.Peripherals.Miscellaneous.Betrusted
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
                .WithFlag(0, name: "ready", valueProviderCallback: _ => true)
                .WithFlag(1, name: "avail", valueProviderCallback: _ => true)
            ;

            Registers.DATA.Define(this)
                .WithValueField(0, 32, FieldMode.Read, valueProviderCallback: _ =>
                {
                    return (uint)rng.Next();
                }, name: "DATA");

            Registers.URANDOM.Define(this)
                .WithValueField(0, 32, FieldMode.Read, valueProviderCallback: _ =>
                {
                    return (uint)rng.Next();
                }, name: "URANDOM");
            Registers.URANDOM_VALID.Define(this)
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => true, name: "URANDOM_VALID");
        }

        private readonly PseudorandomNumberGenerator rng = EmulationManager.Instance.CurrentEmulation.RandomGenerator;
        public GPIO IRQ { get; private set; }

        private enum Registers
        {
            STATUS = 0x0,
            DATA = 0x4,
            URANDOM = 0x8,
            URANDOM_VALID = 0xc,
            EV_STATUS = 0x10,
            EV_PENDING = 0x14,
            EV_ENABLE = 0x18,
        }
    }
}
