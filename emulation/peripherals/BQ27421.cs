//
// Copyright (c) 2010-2020 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//

using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.I2C;
using Antmicro.Renode.Peripherals.Sensor;
using Antmicro.Renode.Utilities;
using Antmicro.Renode.Exceptions;

namespace Antmicro.Renode.Peripherals.Sensors
{
    public class BQ27421 : II2CPeripheral, IProvidesRegisterCollection<ByteRegisterCollection>, ISensor, ITemperatureSensor
    {
        public BQ27421()
        {
            RegistersCollection = new ByteRegisterCollection(this);
            IRQ = new GPIO();
            DefineRegisters();
            Reset();
        }

        private bool firstByte;
        private bool secondByte;

        public void FinishTransmission()
        {
            firstByte = true;
            secondByte = false;
        }

        public void Reset()
        {
            RegistersCollection.Reset();
            IRQ.Set(false);
            regAddress = 0;
            firstByte = true;
            secondByte = false;
            Temperature = 298.1M;
            Voltage = 4000;
            SoC = 90;
            AvgCur = 300;
            RemainingCapacity = 4000;
        }

        public void Write(byte[] data)
        {
            if (data.Length == 0)
            {
                this.Log(LogLevel.Warning, "Unexpected write with no data");
                return;
            }
            if (data.Length == 1)
            {
                if (firstByte)
                {
                    firstByte = false;
                    secondByte = true;
                    return;
                }
                if (secondByte)
                {
                    regAddress = (long)data[0];
                    secondByte = false;
                    return;
                }
                this.Log(LogLevel.Warning, "Unexpected write with one byte of data: {0:X2}", data[0]);
                return;
            }
            uint dataOffset = 0;
            if (firstByte)
            {
                dataOffset += 1;
                firstByte = false;
                secondByte = true;
            }
            if (secondByte)
            {
                secondByte = false;
                regAddress = (long)data[dataOffset];
                dataOffset += 1;
            }

            while (dataOffset < data.Length)
            {
                RegistersCollection.Write((long)regAddress, data[dataOffset]);
                regAddress += 1;
                dataOffset += 1;
            }

            // The first byte contains our address.
            this.Log(LogLevel.Noisy, "Write with {0} bytes of data: {1}", data.Length, Misc.PrettyPrintCollectionHex(data));

            // // The second byte contains the register address
            // regAddress = (Registers)data[1];

            // if(data.Length == 2)
            // {
            //     this.Log(LogLevel.Noisy, "Preparing to read register {0} (0x{0:X})", regAddress);
            //     readyPending.Value = true;
            //     UpdateInterrupts();
            //     return;
            // }
        }

        public byte[] Read(int count)
        {
            this.Log(LogLevel.Noisy, "Reading {0} bytes beginning at register {1} (0x{1:X})", count, (Registers)regAddress);
            var result = new byte[count];
            for (var i = 0; i < count; i++)
            {
                result[i] = RegistersCollection.Read(regAddress + i);
            }
            regAddress += count;
            return result;
        }
        public decimal Temperature { get; set; }
        public uint Voltage;
        public uint SoC;
        public int AvgCur;
        public uint RemainingCapacity;

        public GPIO IRQ { get; }
        public ByteRegisterCollection RegistersCollection { get; }

        private void DefineRegisters()
        {
            Registers.TempLow.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "TEMPERATURE_SENSOR_LOW", valueProviderCallback: _ => ((uint)(Temperature * 10)));
            Registers.TempHigh.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "TEMPERATURE_SENSOR_LOW", valueProviderCallback: _ => ((uint)(Temperature * 10)) >> 8);
            Registers.VoltLow.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "VOLTAGE_LOW", valueProviderCallback: _ => Voltage);
            Registers.VoltHigh.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "VOLTAGE_HIGH", valueProviderCallback: _ => Voltage >> 8);
            Registers.AvgCurLow.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "AVGCUR_LOW", valueProviderCallback: _ => (uint)AvgCur);
            Registers.AvgCurHigh.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "AVGCUR_HIGH", valueProviderCallback: _ => (uint)(AvgCur >> 8));
            Registers.RmLow.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "RM_LOW", valueProviderCallback: _ => RemainingCapacity);
            Registers.RmHigh.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "RM_HIGH", valueProviderCallback: _ => RemainingCapacity >> 8);
            Registers.SocLow.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "SOC_LOW", valueProviderCallback: _ => SoC);
            Registers.SocHigh.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "SOC_HIGH", valueProviderCallback: _ => SoC >> 8);
        }

        private void RegistersAutoIncrement()
        {
        }

        // private void UpdateInterrupts()
        // {
        //     var status = readyEnabled.Value && readyPending.Value;
        //     this.Log(LogLevel.Noisy, "Setting IRQ to {0}", status);
        //     IRQ.Set(status);
        // }

        private byte TwoComplementSignConvert(decimal temp)
        {
            byte tempAsByte = Decimal.ToByte(temp);
            if (temp < 0)
            {
                byte twoComplementTemp = (byte)(~tempAsByte + 1);
                return twoComplementTemp;
            }
            return tempAsByte;
        }

        private IFlagRegisterField readyPending;
        private IFlagRegisterField readyEnabled;
        private IFlagRegisterField highFreqDataRateMode;
        private IValueRegisterField outDataRate;
        private IValueRegisterField fullScale;
        private long regAddress;

        private ushort controlStatus;

        private enum Registers : byte
        {
            Cntl = 0x00,
            // Reserved: 0x01
            TempLow = 0x02,
            TempHigh = 0x03,
            VoltLow = 0x04,
            VoltHigh = 0x05,
            Flag = 0x06,
            // Reserved: 0x07
            NomCap = 0x08,
            // Reserved: 0x09
            FullCap = 0x0A,
            // Reserved: 0x0B
            RmLow = 0x0C,
            RmHigh = 0x0D,
            Fcc = 0x0e,
            // Reserved: 0x0f
            AvgCurLow = 0x10,
            AvgCurHigh = 0x11,
            SbyCur = 0x12,
            // Reserved: 0x13
            MaxCur = 0x14,
            // Reserved: 0x15 - 0x17
            AvgPwr = 0x18,
            // Reserved: 0x19 = 0x1b
            SocLow = 0x1c,
            SocHigh = 0x1d,
            IntTemp = 0x1e,
            // Reserved: 0x1f
            Soh = 0x20,
        }
    }
}
