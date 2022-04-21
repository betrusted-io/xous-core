//
// Copyright (c) 2010-2019 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
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
    public class SusRes : BasicDoubleWordPeripheral, IKnownSize
    {
        public SusRes(Machine machine, ulong periodInMs = 1) : base(machine)
        {
            this.IRQ = new GPIO();
            DefineRegisters();
        }

        private TickTimer EnsureTickTimer()
        {
            if (MainTimer != null)
            {
                return MainTimer;
            }

            foreach (var element in machine.ClockSource.GetAllClockEntries())
            {
                if (element.LocalName == "TickTimer")
                {
                    MainTimer = (TickTimer)element.Owner;
                    this.Log(LogLevel.Info, "Found TickTimer object!");
                    return MainTimer;
                }
            }

            this.Log(LogLevel.Error, "Did not find TickTimer object");
            return null;
        }

        public override void Reset()
        {
            base.Reset();
            irqPending.Value = false;
            RegistersCollection.Reset();
        }

        private void UpdateInterrupts()
        {
            IRQ.Set(this.irqPending.Value && this.irqEnabled.Value);
        }

        public long Size { get { return 4096; } }

        private void DefineRegisters()
        {
            Registers.Control.Define32(this)
                .WithFlag(0, FieldMode.Read | FieldMode.Write, writeCallback: (_, val) =>
                {
                    if (EnsureTickTimer() == null)
                    {
                        return;
                    }
                    MainTimer.paused = val;
                }
                )
                .WithFlag(1, writeCallback: (_, val) =>
                {
                    // Don't update the timer if it's not paused.
                    if ((!EnsureTickTimer()?.paused) ?? false)
                    {
                        return;
                    }
                    if (val) {
                        MainTimer.tickValue = ((long)resumeTime0.Value) | (((long)resumeTime1.Value) << 32);
                    }
                })
            ;

            Registers.Time1.Define32(this)
                .WithValueField(0, 32, FieldMode.Read, name: "Time1", valueProviderCallback: _ =>
                {
                    return EnsureTickTimer()?.Time1() ?? 0;
                })
            ;

            Registers.Time0.Define32(this)
                .WithValueField(0, 32, FieldMode.Read, name: "Time0", valueProviderCallback: _ =>
                {
                    return EnsureTickTimer()?.Time0() ?? 0;
                })
            ;

            Registers.ResumeTime0.Define32(this)
                .WithValueField(0, 32, out resumeTime0)
            ;

            Registers.ResumeTime1.Define32(this)
                .WithValueField(0, 32, out resumeTime1)
            ;

            Registers.Status.Define32(this)
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => EnsureTickTimer()?.paused ?? false, name: "paused")
            ;

            Registers.State.Define32(this)
                .WithFlag(0, out isResume)
                .WithFlag(1, out wasForced)
            ;

            Registers.Powerdown.Define32(this)
                .WithFlag(0, FieldMode.Read | FieldMode.Write, changeCallback: (_, val) => {
                    if (val) {
                        this.Log(LogLevel.Info, "Pausing and resetting machine");
                        machine.LocalTimeSource.ExecuteInNearestSyncedState(ts => {
                            machine.Pause();
                            machine.Reset();
                        });
                    }
                })
            ;

            Registers.Wfi.Define32(this)
                .WithFlag(0, name: "override")
            ;

            Registers.Interrupt.Define32(this)
                .WithFlag(0, FieldMode.Write, writeCallback: (_, val) =>
                {
                    if (val)
                    {
                        irqPending.Value = true;
                    }
                    UpdateInterrupts();
                })
            ;

            Registers.EventStatus.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "EV_STATUS", valueProviderCallback: _ => false)
            ;

            Registers.EventPending.Define32(this)
                .WithFlag(0, out irqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "EV_PENDING", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EventEnable.Define32(this)
                .WithFlag(0, out irqEnabled, name: "EV_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
            ;

        }

        TickTimer MainTimer;

        private IFlagRegisterField irqEnabled;
        private IFlagRegisterField irqPending;
        private IValueRegisterField resumeTime0;
        private IValueRegisterField resumeTime1;
        private IFlagRegisterField timerPaused;
        private IFlagRegisterField isResume;
        private IFlagRegisterField wasForced;

        public GPIO IRQ { get; private set; }

        private enum Registers
        {
            Control = 0x00,
            ResumeTime1 = 0x04,
            ResumeTime0 = 0x08,
            Time1 = 0x0c,
            Time0 = 0x10,
            Status = 0x14,
            State = 0x18,
            Powerdown = 0x1c,
            Wfi = 0x20,
            Interrupt = 0x24,
            EventStatus = 0x28,
            EventPending = 0x2c,
            EventEnable = 0x30,
        }
    }
}
