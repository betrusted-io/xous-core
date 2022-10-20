//
// Copyright (c) 2010-2021 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.

using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;

namespace Antmicro.Renode.Peripherals.GPIOPort.Betrusted
{
    public class BtEvents : BaseGPIOPort, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IDoubleWordPeripheral, IKnownSize
    {
        public BtEvents(Machine machine) : base(machine, NumberOfPins)
        {
            IRQ = new GPIO();
            Pins = new Pin[NumberOfPins];
            for (var i = 0; i < Pins.Length; i++)
            {
                Pins[i] = new Pin(this, i);
            }

            RegistersCollection = new DoubleWordRegisterCollection(this);
            DefineRegisters();
        }

        public override void Reset()
        {
            base.Reset();

            RegistersCollection.Reset();

            foreach (var pin in Pins)
            {
                pin.Reset();
            }

            ComIntPending.Value = false;
            RtcIntPending.Value = false;
            ComIntEnable.Value = false;
            RtcIntEnable.Value = false;
            UpdateInterrupts();
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            RegistersCollection.Write(offset, value);
        }

        public override void OnGPIO(int number, bool value)
        {
            // These interrupts fire on the rising edge
            if (number == 0 && !Pins[number].Value && value) {
                ComIntPending.Value = true;
            }
            if (number == 1 && !Pins[number].Value && value) {
                RtcIntPending.Value = true;
            }
            base.OnGPIO(number, value);
            if (CheckPinNumber(number))
            {
                PinChanged?.Invoke(Pins[number], value);
            }
            UpdateInterrupts();
        }

        public DoubleWordRegisterCollection RegistersCollection { get; }
        public Pin[] Pins { get; }
        public GPIO IRQ { get; private set; }

        public long Size => 0x1000;

        public event Action<Pin, bool> PinChanged;
        public event Action Detect;

        private void DefineRegisters()
        {
            Registers.EvStatus.Define32(this)
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => Pins[0].Value, name: "com_int")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: _ => Pins[1].Value, name: "rtc_int")
            ;

            Registers.EvPending.Define32(this)
                .WithFlag(0, out ComIntPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "com_int", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out RtcIntPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "rtc_int", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EvEnable.Define32(this)
                .WithFlag(0, out ComIntEnable, FieldMode.Read | FieldMode.Write, name: "com_int", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out RtcIntEnable, FieldMode.Read | FieldMode.Write, name: "rtc_int", changeCallback: (_, __) => UpdateInterrupts())
            ;
        }

        private void UpdateInterrupts()
        {
            IRQ.Set((ComIntPending.Value && ComIntEnable.Value)
                 || (RtcIntPending.Value && RtcIntEnable.Value));
        }

        private bool detectState;
        private IFlagRegisterField ComIntPending;
        private IFlagRegisterField RtcIntPending;
        private IFlagRegisterField ComIntEnable;
        private IFlagRegisterField RtcIntEnable;

        private const int NumberOfPins = 2;

        public class Pin
        {
            public Pin(BtEvents parent, int id)
            {
                this.Parent = parent;
                this.Id = id;
            }

            public void Reset()
            {
                Parent.Connections[Id].Set(false);
            }

            public bool Value
            {
                get
                {
                    return RawValue;
                }
                set
                {
                    Parent.NoisyLog("Setting pin {0} to {1}", Id, value);
                    Parent.Connections[Id].Set(value);
                    Parent.State[Id] = value;
                    Parent.PinChanged(this, value);
                    Parent.UpdateInterrupts();
                }
            }

            // This property is needed as the pull-up and pull-down should be
            // able to override Pin state not matter Direction it is set to.
            public bool RawValue
            {
                get
                {
                    return Parent.State[Id];
                }
                set
                {
                    Parent.State[Id] = value;
                }
            }

            public BtEvents Parent { get; }
            public int Id { get; }
        }

        private enum Registers
        {
            EvStatus = 0x00,
            EvPending = 0x04,
            EvEnable = 0x08,
        }
    }
}
