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
    public class BetrustedRNGServer : BasicDoubleWordPeripheral, IKnownSize
    {
        public BetrustedRNGServer(Machine machine) : base(machine)
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
                    return (1 << 0) | (1 << 21);
                }, name: "STATUS");

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
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => { return true; }, name: "URANDOM_VALID");
        }

        private readonly PseudorandomNumberGenerator rng = EmulationManager.Instance.CurrentEmulation.RandomGenerator;
        public GPIO IRQ { get; private set; }

        private enum Registers
        {
            CONTROL = 0x0,
            DATA = 0x4,
            STATUS = 0x8,
            AV_CONFIG = 0xc,
            RO_CONFIG = 0x10,

            READY = 0xc4,
            EV_STATUS = 0xc8,
            EV_PENDING = 0xcc,
            EV_ENABLE = 0xd0,
            URANDOM = 0xdc,
            URANDOM_VALID = 0xe0,
            TEST = 0xf8,
        }
    }
}
