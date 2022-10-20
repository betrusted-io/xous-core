//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Miscellaneous.Betrusted
{
    public class EcPower : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public EcPower(Machine machine)
        {
            Machine = machine;
            RegistersCollection = new DoubleWordRegisterCollection(this);

            Registers.POWER.Define32(this)
                .WithFlag(0, out Self, FieldMode.Read | FieldMode.Write, name: "SELF")
                .WithFlag(1, out SocOn, FieldMode.Read | FieldMode.Write, name: "SOC_ON")
                .WithFlag(2, out KbdDrive, FieldMode.Read | FieldMode.Write, name: "KBDDRIVE")
            ;
            Registers.STATS.Define32(this)
                .WithFlag(0, out State, FieldMode.Read, name: "STATE")
                .WithValueField(1, 2, out MonKey, FieldMode.Read, name: "MONKEY")
            ;

            Reset();
        }

        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public long Size { get { return 4096; } }
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        public void Reset()
        {
            Self.Value = true;
            SocOn.Value = true;
            KbdDrive.Value = false;

            // Keep the SoC powered "on" to work around power emulation issues
            State.Value = true;
            MonKey.Value = 0;
        }

        private readonly Machine Machine;
        private IFlagRegisterField Self;
        private IFlagRegisterField SocOn;
        private IFlagRegisterField KbdDrive;

        private IFlagRegisterField State;
        private IValueRegisterField MonKey;

        private enum Registers
        {
            POWER = 0x0,
            STATS = 0x04,
        }
    }
}
