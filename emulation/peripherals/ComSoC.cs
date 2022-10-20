//
// Copyright (c) 2010-2021 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Core.Structure.Registers;

namespace Antmicro.Renode.Peripherals.SPI.Betrusted
{
    public class BetrustedSocCom : NullRegistrationPointPeripheralContainer<IComController>, IDoubleWordPeripheral,
                                IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize, IGPIOReceiver
    {
        public BetrustedSocCom(Machine machine) : base(machine)
        {
            IRQ = new GPIO();
            HOLD = new GPIO();
            EC_INTERRUPT = new GPIO();
            RegistersCollection = new DoubleWordRegisterCollection(this);

            Registers.Tx.Define32(this)
                .WithValueField(0, 16, FieldMode.Write, writeCallback: (_, val) =>
                {
                    // Skip transaction if HOLD is set
                    // if (AutoHold.Value && (HOLD && HOLD.IsSet)
                    // {
                    //     return;
                    // }
                    Rx.Value = (uint)RegisteredPeripheral.Transmit((ushort)val);
                    RegisteredPeripheral.FinishTransmission();
                    // machine.Log(LogLevel.Noisy, "SoC sent {0:X4} received {1:X4}", val, Rx.Value);
                }, name: "TX")
            ;

            Registers.Rx.Define32(this)
                .WithValueField(0, 16, out Rx, FieldMode.Read, name: "RX")
            ;

            Registers.Control.Define32(this)
                .WithFlag(0, out IntEna, FieldMode.Read | FieldMode.Write, name: "IntEna")
                .WithFlag(1, out AutoHold, FieldMode.Read | FieldMode.Write, name: "AutoHold")
            ;
            Registers.Status.Define32(this)
                .WithFlag(0, out Tip, FieldMode.Read, name: "Tip")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: (_) => HOLD.IsSet, name: "Hold")
            ;

            Registers.EvStatus.Define32(this)
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => EC_INTERRUPT.IsSet, name: "SpiInt")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: _ => HOLD.IsSet, name: "SpiHold")
            ;

            Registers.EvPending.Define32(this)
                .WithFlag(0, out SpiIntPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "SpiInt", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out SpiHoldPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "SpiHold", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EvEnable.Define32(this)
                .WithFlag(0, out SpiIntEnable, FieldMode.Read | FieldMode.Write, name: "SpiInt", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out SpiHoldEnable, FieldMode.Read | FieldMode.Write, name: "SpiHold", changeCallback: (_, __) => UpdateInterrupts())
            ;

            HOLD.Connect(this, 0);
            EC_INTERRUPT.Connect(this, 1);
            Reset();
        }

        public void OnGPIO(int number, bool value)
        {
            if (number == 1)
            {
                // this.Log(LogLevel.Error, "Setting IRQ GPIO to {0}", value ? "1" : "0");
                EC_INTERRUPT.Set(value);
            } else if (number == 0) {
                // this.Log(LogLevel.Error, "Setting HOLD GPIO to {0}", value ? "1" : "0");
                HOLD.Set(value);
            }
            UpdateInterrupts();
        }

        private void UpdateInterrupts()
        {
            if (HOLD.IsSet)
            {
                SpiHoldPending.Value = true;
            }
            if (EC_INTERRUPT.IsSet)
            {
                SpiIntPending.Value = true;
            }

            IRQ.Set((SpiIntPending.Value && SpiIntEnable.Value)
                 || (SpiHoldPending.Value && SpiHoldEnable.Value));
        }

        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public override void Reset()
        {
            SpiIntPending.Value = false;
            SpiHoldPending.Value = false;
            SpiIntEnable.Value = false;
            SpiHoldEnable.Value = false;

            Tip.Value = false;
            EC_INTERRUPT.Set(false);
            HOLD.Set(true);
        }


        public long Size { get { return 0x100; } }

        public GPIO IRQ { get; private set; }
        public GPIO HOLD { get; private set; }
        public GPIO EC_INTERRUPT { get; private set; }

        private IFlagRegisterField IntEna;
        private IFlagRegisterField AutoHold;
        private IFlagRegisterField Tip;
        private IValueRegisterField Rx;

        // Interrupts
        private IFlagRegisterField SpiIntPending;
        private IFlagRegisterField SpiHoldPending;
        private IFlagRegisterField SpiIntEnable;
        private IFlagRegisterField SpiHoldEnable;

        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        private enum Registers
        {
            Tx = 0x00,
            Rx = 0x04,
            Control = 0x08,
            Status = 0x0c,
            EvStatus = 0x10,
            EvPending = 0x14,
            EvEnable = 0x18,
        }
    }
}
