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
    public class BetrustedRNGServer : BasicDoubleWordPeripheral, IKnownSize
    {
        public BetrustedRNGServer(Machine machine) : base(machine)
        {
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
        }

        private readonly PseudorandomNumberGenerator rng = EmulationManager.Instance.CurrentEmulation.RandomGenerator;

        private enum Registers
        {
            CONTROL = 0x0,
            DATA = 0x4,
            STATUS = 0x8,
            AV_CONFIG = 0xc,
            RO_CONFIG = 0x10,
            ERRORS = 0x14,
            EV_STATUS = 0x18,
            EV_PENDING = 0x1c,
            EV_ENABLE = 0x20,
        }
    }
}
