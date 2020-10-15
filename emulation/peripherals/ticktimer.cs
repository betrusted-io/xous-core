//
// Copyright (c) 2010-2019 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Utilities;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Time;
using Antmicro.Renode.Peripherals.Bus;
using System.Threading;

namespace Antmicro.Renode.Peripherals.Timers
{
    // this is a model of LiteX timer in the Betrusted configuration:
    // * width: 32 bits
    // * csr data width: 32 bit
    [AllowedTranslations(AllowedTranslation.ByteToDoubleWord)]
    public class TickTimer : BasicDoubleWordPeripheral, IKnownSize
    {
        public TickTimer(Machine machine, ulong periodInMs) : base(machine)
        {
            machine.ClockSource.AddClockEntry(new ClockEntry(periodInMs, ClockEntry.FrequencyToRatio(this, 1000), OnTick, this, "TickTimer"));
            DefineRegisters();
        }

        public override void Reset()
        {
            base.Reset();
            tickValue = 0;
            paused = true;
        }

        private void OnTick()
        {
            if (!paused) {
                Interlocked.Increment(ref tickValue);
            }
        }

        public long Size { get { return  0x20; }}

        private void DefineRegisters()
        {
            Registers.Control.Define32(this)
                .WithValueField(0, 32, name: "CONTROL", writeCallback: (_, val) =>
                {
                    if ((val & 1) != 0) {
                        tickValue = 0;
                    }
                    paused = (val & 2) != 0;
                });
            ;

            Registers.Time1.Define32(this)
                .WithValueField(0, 32, FieldMode.Read, name: "Time1", valueProviderCallback: _ =>
                {
                    return (uint) (tickValue >> 32);
                })
            ;

            Registers.Time0.Define32(this)
                .WithValueField(0, 32, FieldMode.Read, name: "Time0", valueProviderCallback: _ =>
                {
                    return (uint) (tickValue >> 0);
                })
            ;
        }

        bool paused;
        long tickValue;

        private enum Registers
        {
            Control = 0x00,
            Time1 = 0x04,
            Time0 = 0x08,
        }
    }
}
