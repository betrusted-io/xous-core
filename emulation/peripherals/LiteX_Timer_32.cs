//
// Copyright (c) 2010-2019 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Timers.Betrusted
{
    // this is a model of LiteX timer in the Betrusted configuration:
    // * width: 32 bits
    // * csr data width: 32 bit
    [AllowedTranslations(AllowedTranslation.ByteToDoubleWord)]
    public class LiteX_Timer_32 : BasicDoubleWordPeripheral, IKnownSize
    {
        public LiteX_Timer_32(Machine machine, long frequency) : base(machine)
        {
            IRQ = new GPIO();
            innerTimer = new LimitTimer(machine.ClockSource, frequency, this, "LiteXTimer32", eventEnabled: true, autoUpdate: true);
            innerTimer.LimitReached += delegate
            {
                this.Log(LogLevel.Noisy, "Limit reached");
                irqPending.Value = true;
                UpdateInterrupts();

                if(reloadValue == 0)
                {
                    this.Log(LogLevel.Noisy, "No realod value - disabling the timer");
                    innerTimer.Enabled = false;
                }
                innerTimer.Limit = reloadValue;
            };
            DefineRegisters();
        }

        public override void Reset()
        {
            base.Reset();
            innerTimer.Reset();
            latchedValue = 0;
            loadValue = 0;
            reloadValue = 0;
            RegistersCollection.Reset();

            UpdateInterrupts();
        }

        public GPIO IRQ {
            get;
            private set;
         }

        public long Size { get { return  0x20; }}

        private void DefineRegisters()
        {
            Registers.Load.Define32(this)
                .WithValueField(0, 32, name: "LOAD", writeCallback: (_, val) =>
                {
                    loadValue = val;
                });
            ;

            Registers.Reload.Define32(this)
                .WithValueField(0, 32, name: "RELOAD", writeCallback: (_, val) =>
                {
                    reloadValue = val;
                });
            ;

            Registers.TimerEnable.Define32(this)
                .WithFlag(0, name: "ENABLE", writeCallback: (_, val) =>
                {
                    if(innerTimer.Enabled == val)
                    {
                        return;
                    }

                    if(val)
                    {
                        innerTimer.Limit = loadValue;
                        this.Log(LogLevel.Noisy, "Enabling timer. Load value: 0x{0:X}, reload value: 0x{1:X}", loadValue, reloadValue);
                    }

                    innerTimer.Enabled = val;
                })
            ;

            Registers.EventStatus.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "EV_STATUS", valueProviderCallback: _ => innerTimer.Value == 0)
            ;

            Registers.EventPending.Define32(this)
                .WithFlag(0, out irqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "EV_PENDING", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EventEnable.Define32(this)
                .WithFlag(0, out irqEnabled, name: "EV_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
            ;
        }

        private void UpdateInterrupts()
        {
            this.Log(LogLevel.Noisy, "Setting IRQ: {0}", irqPending.Value && irqEnabled.Value);
            IRQ.Set(irqPending.Value && irqEnabled.Value);
        }

        private IFlagRegisterField irqEnabled;
        private IFlagRegisterField irqPending;

        private uint latchedValue;
        private ulong loadValue;
        private ulong reloadValue;

        private readonly LimitTimer innerTimer;

        private enum Registers
        {
            Load = 0x00,

            Reload = 0x04,

            TimerEnable = 0x08,

            EventStatus = 0x0c,
            EventPending = 0x10,
            EventEnable = 0x14
        }
    }
}
