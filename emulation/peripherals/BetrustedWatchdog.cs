//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Time;
using System.Threading;
using System.Collections.Generic;
using System;

namespace Antmicro.Renode.Peripherals.Timers.Betrusted
{
    public sealed class BetrustedWatchdog : BasicDoubleWordPeripheral, IKnownSize
    {
        public BetrustedWatchdog(Machine machine) : base(machine)
        {
            machine.ClockSource.AddClockEntry(new ClockEntry(10, 1000, OnTick, this, String.Empty));
            var registersMap = new Dictionary<long, DoubleWordRegister>()
            {
                {(long)Registers.Watchdog, new DoubleWordRegister(this)
                    .WithFlag(1, FieldMode.Read | FieldMode.Write, (_, enable) => {
                        if (enable) {
                            enabled = true;
                        }
                    })
                    .WithFlag(0, FieldMode.Read | FieldMode.Write, writeCallback: (_, should_reset) => {
                        this.Log(LogLevel.Debug, "Resetting watchdog timer");
                        Interlocked.Exchange(ref counter, 0);
                    })
                },
                {(long)Registers.Period, new DoubleWordRegister(this)
                    .WithValueField(0, 32, FieldMode.Read | FieldMode.Write, (_, period) => {
                        this.reset_target = XilinxClockToMs(period);
                    })
                },
                {(long)Registers.State, new DoubleWordRegister(this)
                    .WithFlag(0, FieldMode.Read, valueProviderCallback: (_) => enabled, name: "ENABLED")
                    .WithFlag(1, FieldMode.Read, valueProviderCallback: (_) => false, name: "ARMED1")
                    .WithFlag(2, FieldMode.Read, valueProviderCallback: (_) => false, name: "ARMED2")
                    .WithFlag(3, FieldMode.Read, valueProviderCallback: (_) => false, name: "DISARMED")
                },
            };

            registers = new DoubleWordRegisterCollection(this, registersMap);
            Reset();
        }
        private void OnTick()
        {
            Interlocked.Increment(ref counter);
            if (((uint)counter) >= this.reset_target)
            {
                this.Log(LogLevel.Error, "Watchdog timer expired -- requesting system reset");
                this.machine.RequestReset();
            }
        }

        public override uint ReadDoubleWord(long offset)
        {
            return registers.Read(offset);
        }

        public override void WriteDoubleWord(long offset, uint value)
        {
            registers.Write(offset, value);
        }
        public override void Reset()
        {
            registers.Reset();
            enabled = false;
            counter = 0;
            reset_target = XilinxClockToMs(325000000);
        }

        // Convert "approximately 65MHz" ticks to milliseconds
        private uint XilinxClockToMs(ulong xilinx_clocks)
        {
            uint adjusted = (uint)((xilinx_clocks * 100) / 65000000);
            this.Log(LogLevel.Debug, "Watchdog timer set for {0}. Will expire after {1} msec", xilinx_clocks, adjusted * 10);
            return adjusted;
        }

        public long Size
        {
            get
            {
                return 0xC;
            }
        }

        private int counter;
        private uint reset_target;
        private bool enabled;
        private readonly DoubleWordRegisterCollection registers;
        private enum Registers
        {
            Watchdog = 0x0,
            Period = 0x4,
            State = 0x8,
        }
    }
}
