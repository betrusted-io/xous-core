//
// Copyright (c) 2010-2019 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Time;
using Antmicro.Renode.Peripherals.Bus;
using System.Threading;

namespace Antmicro.Renode.Peripherals.Timers.Betrusted
{
    // this is a model of LiteX timer in the Betrusted configuration:
    // * width: 32 bits
    // * csr data width: 32 bit
    [AllowedTranslations(AllowedTranslation.ByteToDoubleWord)]
    public class TickTimer : BasicDoubleWordPeripheral, IKnownSize
    {
        public TickTimer(Machine machine, ulong periodInMs = 1) : base(machine)
        {
            machine.ClockSource.AddClockEntry(new ClockEntry(periodInMs, 1000, OnTick, this, "TickTimer"));
            this.IRQ = new GPIO();
            DefineRegisters();
        }

        public override void Reset()
        {
            base.Reset();
            tickValue = 0;
            paused = true;
            msleepTarget = 0;
            RegistersCollection.Reset();
        }

        private void OnTick()
        {
            if (!paused)
            {
                this.irqStatus = Interlocked.Increment(ref tickValue) >= (long)this.msleepTarget;
                this.UpdateInterrupts();
            }
        }

        public uint Time0()
        {
            return (uint)(tickValue >> 0);
        }

        public uint Time1()
        {
            return (uint)(tickValue >> 32);
        }

        private void UpdateInterrupts()
        {
            if ((Interlocked.Read(ref this.tickValue) >= (long)this.msleepTarget) && this.irqEnabled.Value)
            {
                this.irqPending.Value = true;
            }
            this.Log(LogLevel.Noisy, "Setting IRQ: {0} because tickValue {1} >= msleepTarget {2} ({3}) and irqEnabled = {4}",
                    irqPending.Value && this.irqEnabled.Value,
                    this.tickValue,
                    this.msleepTarget,
                    this.tickValue >= (long)this.msleepTarget, this.irqEnabled.Value);
            IRQ.Set(this.irqPending.Value && this.irqEnabled.Value);
        }

        public long Size { get { return 0x20; } }

        private void DefineRegisters()
        {
            Registers.Control.Define32(this)
                .WithValueField(0, 32, name: "CONTROL", writeCallback: (_, val) =>
                {
                    if ((val & 1) != 0)
                    {
                        tickValue = 0;
                    }
                    paused = (val & 2) != 0;
                });
            ;

            Registers.Time1.Define32(this)
                .WithValueField(0, 32, FieldMode.Read, name: "Time1", valueProviderCallback: _ =>
                {
                    return Time1();
                })
            ;

            Registers.Time0.Define32(this)
                .WithValueField(0, 32, FieldMode.Read, name: "Time0", valueProviderCallback: _ =>
                {
                    return Time0();
                })
            ;

            Registers.MsleepTarget1.Define32(this)
                .WithValueField(0, 32, name: "MsleepTarget1", writeCallback: (_, value) =>
                {
                    this.msleepTarget = (this.msleepTarget & 0x00000000ffffffff) | (((ulong)value) << 32);
                    this.Log(LogLevel.Noisy, "Setting MsleepTarget1: {0}, sleep target now: {1}", value, this.msleepTarget);
                },
                valueProviderCallback: _ =>
                {
                    return (uint)(this.msleepTarget >> 32);
                })
            ;

            Registers.MsleepTarget0.Define32(this)
                .WithValueField(0, 32, name: "MsleepTarget0",
                writeCallback: (_, value) =>
                {
                    this.msleepTarget = (this.msleepTarget & 0xffffffff00000000) | (value & 0xffffffff);
                    this.Log(LogLevel.Noisy, "Setting MsleepTarget0: {0}, sleep target now: {1}", value, this.msleepTarget);
                },
                valueProviderCallback: _ =>
                {
                    return (uint)(this.msleepTarget >> 0);
                })
                ;

            Registers.EventStatus.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "EV_STATUS", valueProviderCallback: _ => irqStatus)
            ;

            Registers.EventPending.Define32(this)
                .WithFlag(0, out irqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "EV_PENDING", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EventEnable.Define32(this)
                .WithFlag(0, out irqEnabled, name: "EV_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
            ;

        }

        private IFlagRegisterField irqEnabled;
        private IFlagRegisterField irqPending;
        private bool irqStatus;

        public bool paused;
        public long tickValue;
        public ulong msleepTarget;
        public GPIO IRQ { get; private set; }

        private enum Registers
        {
            Control = 0x00,
            Time1 = 0x04,
            Time0 = 0x08,
            MsleepTarget1 = 0x0c,
            MsleepTarget0 = 0x10,
            EventStatus = 0x14,
            EventPending = 0x18,
            EventEnable = 0x1c,
        }
    }
}
