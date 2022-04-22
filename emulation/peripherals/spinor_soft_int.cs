//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using System.Threading;
using System.Text;
using System.Linq;
using System.Globalization;
using System.Numerics;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{

    public class SpinorSoftInt : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public SpinorSoftInt(Machine machine)
        {
            this.RegistersCollection = new DoubleWordRegisterCollection(this);
            this.IRQ = new GPIO();
            DefineRegisters();
            Reset();
        }

        private void DefineRegisters()
        {
            Registers.EV_STATUS.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "SOFTINT", valueProviderCallback: _ => softintStatus)
            ;

            Registers.EV_PENDING.Define32(this)
                .WithFlag(0, out this.softintPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "SOFTINT", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EV_ENABLE.Define32(this)
                .WithFlag(0, out this.softintEnabled, name: "SOFTINT", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.SOFTINT.Define32(this)
                .WithFlag(0, writeCallback: (_, val) => { if (val) { this.softintStatus = true; UpdateInterrupts(); this.softintStatus = false; } })
            ;
        }

        private void UpdateInterrupts()
        {
            if (this.softintStatus)
            {
                this.softintPending.Value = true;
            }
            IRQ.Set(this.softintPending.Value && this.softintEnabled.Value);
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
            this.softintStatus = false;
            this.softintPending.Value = false;
            this.softintEnabled.Value = false;
            RegistersCollection.Reset();
        }

        public GPIO IRQ { get; private set; }

        private IFlagRegisterField softintEnabled;
        private IFlagRegisterField softintPending;
        private bool softintStatus;

        private enum Registers
        {
            EV_STATUS = 0x00,
            EV_PENDING = 0x04,
            EV_ENABLE = 0x08,
            SOFTINT = 0x0c
        }
    }
}
