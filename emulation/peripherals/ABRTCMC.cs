//
// Copyright (c) 2010-2020 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//

using System;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Peripherals.I2C;
using Antmicro.Renode.Peripherals.Sensor;

namespace Antmicro.Renode.Peripherals.Timers.Betrusted
{
    public class ABRTCMC : II2CPeripheral, IProvidesRegisterCollection<ByteRegisterCollection>, ISensor
    {
        public ABRTCMC()
        {
            RegistersCollection = new ByteRegisterCollection(this);
            addressAutoIncrement = true;
            isFirstByte = true;

            Registers.Control0.Define(this)
                .WithFlag(7, FieldMode.Read, valueProviderCallback: _ => false, name: "CAP")
                .WithFlag(6, FieldMode.Read, valueProviderCallback: _ => false, name: "N")
                .WithFlag(5, FieldMode.Read, valueProviderCallback: _ => false, name: "STOP")
                .WithFlag(4, FieldMode.Read, valueProviderCallback: _ => false, name: "SR")
                .WithFlag(3, out twelveOrTwentyFourHourMode, name: "12_24")
                .WithFlag(2, FieldMode.Read, valueProviderCallback: _ => false, name: "SIE")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: _ => false, name: "AIE")
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => false, name: "CIE")
            ;

            Registers.Control1.Define(this)
                .WithFlag(7, FieldMode.Read, valueProviderCallback: _ => false, name: "WTAF")
                .WithFlag(6, FieldMode.Read, valueProviderCallback: _ => false, name: "CTAF")
                .WithFlag(5, FieldMode.Read, valueProviderCallback: _ => false, name: "CTBF")
                .WithFlag(4, FieldMode.Read, valueProviderCallback: _ => false, name: "SF")
                .WithFlag(3, FieldMode.Read, valueProviderCallback: _ => false, name: "AF")
                .WithFlag(2, FieldMode.Read, valueProviderCallback: _ => false, name: "WTAIE")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: _ => false, name: "CTAIE")
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => false, name: "CTBIE")
            ;

            Registers.Control2.Define(this)
                .WithFlag(7, FieldMode.Read, valueProviderCallback: _ => false, name: "PM2")
                .WithFlag(6, FieldMode.Read, valueProviderCallback: _ => false, name: "PM1")
                .WithFlag(5, FieldMode.Read, valueProviderCallback: _ => false, name: "PM0")
                .WithFlag(4, FieldMode.Read, valueProviderCallback: _ => false, name: "X")
                .WithFlag(3, FieldMode.Read, valueProviderCallback: _ => false, name: "BSF")
                .WithFlag(2, FieldMode.Read, valueProviderCallback: _ => false, name: "BLF")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: _ => false, name: "BSEI")
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => false, name: "BLIE")
            ;

            Registers.Seconds.Define(this)
                .WithFlag(7, FieldMode.Read, valueProviderCallback: _ => false, name: "OS")
                .WithValueField(0, 7, FieldMode.Read, name: "SECONDS", valueProviderCallback: _ => IntToBcd(DateTime.Now.Second))
            ;

            Registers.Minutes.Define(this)
                .WithReservedBits(7, 1)
                .WithValueField(0, 7, FieldMode.Read, name: "MINUTES", valueProviderCallback: _ => IntToBcd(DateTime.Now.Minute))
            ;

            Registers.Hours.Define(this)
                .WithReservedBits(6, 2)
                .WithValueField(0, 6, FieldMode.Read, name: "HOURS", valueProviderCallback: _ => IntToBcd(DateTime.Now.Hour))
            ;

            Registers.Days.Define(this)
                .WithReservedBits(6, 2)
                .WithValueField(0, 6, FieldMode.Read, name: "DAYS", valueProviderCallback: _ => IntToBcd(DateTime.Now.Day))
            ;

            Registers.Weekdays.Define(this)
                .WithReservedBits(5, 1)
                .WithValueField(0, 5, FieldMode.Read, name: "WEEKDAYS", valueProviderCallback: _ => IntToBcd(1+(int)DateTime.Now.DayOfWeek))
            ;

            Registers.Months.Define(this)
                .WithReservedBits(5, 3)
                .WithValueField(0, 5, FieldMode.Read, name: "MONTHS", valueProviderCallback: _ => IntToBcd(DateTime.Now.Month))
            ;

            Registers.Years.Define(this)
                .WithValueField(0, 7, FieldMode.Read, name: "YEARS", valueProviderCallback: _ => IntToBcd(DateTime.Now.Year % 100))
            ;

            Registers.TimerAClock.Define(this)
                .WithValueField(0, 7, name: "TIMERACLOCK")
            ;

            Registers.TimerA.Define(this)
                .WithValueField(0, 7, name: "TIMERA")
            ;
        }

        public void Reset()
        {
            RegistersCollection.Reset();

            address = 0;
            addressAutoIncrement = true;
            isFirstByte = true;
            isSecondByte = false;
        }

        private uint IntToBcd(int input) {
            int bcd = 0;
            for (int digit = 0; digit < 3; ++digit) {
                int nibble = input % 10;
                bcd |= nibble << (digit * 4);
                input /= 10;
            }
            return (uint) bcd;
        }

        public void FinishTransmission()
        {
            // this.Log(LogLevel.Error, "In slave FinishTransmission()");
            isFirstByte = true;
            isSecondByte = false;
        }

        public void Write(byte[] data)
        {
            // this.Log(LogLevel.Warning, "Written {0} bytes: {1}", data.Length, Misc.PrettyPrintCollectionHex(data));
            foreach (var b in data)
            {
                WriteByte(b);
            }
        }

        public void WriteByte(byte b)
        {
            if (isFirstByte) {
                isFirstByte = false;
                isSecondByte = true;
                return;
            }
            if (isSecondByte) {
                isSecondByte = false;
                address = b;
                return;
            }
            // this.Log(LogLevel.Warning, "RTC writing byte 0x{0:x} to register {1} (0x{1:x})", b, address);
            RegistersCollection.Write(address, b);
            TryIncrementAddress();
        }

        public byte[] Read(int count = 1)
        {
            var result = RegistersCollection.Read(address);
            // this.Log(LogLevel.Warning, "Reading register {1} (0x{1:x}) from device: 0x{0:x}", result, (Registers)address);
            TryIncrementAddress();

            return new byte[] { result };
        }

        public ByteRegisterCollection RegistersCollection { get; }

        private void TryIncrementAddress()
        {
            if (!addressAutoIncrement)
            {
                return;
            }
            address = (byte)((address + 1) % 0x14);
        }

        private byte address;
        private bool addressAutoIncrement;
        private bool isFirstByte;
        private bool isSecondByte;

        private readonly IFlagRegisterField twelveOrTwentyFourHourMode;

        private enum Registers
        {
            Control0 = 0x00,
            Control1 = 0x01,
            Control2 = 0x02,
            Seconds = 0x03,
            Minutes = 0x04,
            Hours = 0x05,
            Days = 0x06,
            Weekdays = 0x07,
            Months = 0x08,
            Years = 0x09,
            MinuteAlarm = 0x0A,
            HourAlarm = 0x0B,
            DayAlarm = 0x0C,
            WeekdayAlarm = 0x0D,
            FrequencyOffset = 0x0E,
            TimerClockOut = 0x0F,
            TimerAClock = 0x10,
            TimerA = 0x11,
            TimerBClock = 0x12,
            TimerB = 0x13,
        }
    }
}
