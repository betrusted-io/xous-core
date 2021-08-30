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
        }

        public void FinishTransmission(){}

        public void Reset()
        {
            RegistersCollection.Reset();
            IRQ.Set(false);
            regAddress = 0;
        }

        public void Write(byte[] data)
        {
            if(data.Length == 0 || data.Length == 1)
            {
                this.Log(LogLevel.Warning, "Unexpected write with no data");
                return;
            }

            // The first byte contains our address.
            this.Log(LogLevel.Noisy, "Write with {0} bytes of data: {1}", data.Length, Misc.PrettyPrintCollectionHex(data));

            // The second byte contains the register address
            regAddress = (Registers)data[1];

            if(data.Length == 2)
            {
                this.Log(LogLevel.Noisy, "Preparing to read register {0} (0x{0:X})", regAddress);
                readyPending.Value = true;
                UpdateInterrupts();
                return;
            }

            // // skip the two bytes as they contain our address and the register address
            // foreach(var b in data.Skip(2))
            // {
            //     this.Log(LogLevel.Noisy, "Writing 0x{0:X} to register {1} (0x{1:X})", b, regAddress);
            //     RegistersCollection.Write((byte)regAddress, b);
            // }
            if (data[1] == 0x00) {
                var cmd = (data[2] << 0) | (data[3] << 8);
                this.Log(LogLevel.Noisy, "Issuing control command (0x{0:X})", cmd);
                switch (cmd) {
                    case 0x0000: response = controlStatus; break;
                    case 0x0012: controlStatus &= 0xffbf; break;
                    default: this.Log(LogLevel.Error, "Unhandled Control code: 0x{0:X}", cmd); break;
                }
            }
        }

        public byte[] Read(int count)
        {
            this.Log(LogLevel.Error, "Reading {0} bytes from register {1} (0x{1:X})", count, regAddress);
            var result = new byte[count];
            readyPending.Value = false;
            UpdateInterrupts();
            if (count >= 1) {
                result[0] = (byte)(response & 0xff);
            }
            if (count >= 2) {
                result[1] = (byte)((response >> 8) & 0xff);
            }
            // for(var i = 0; i < result.Length; i++)
            // {
            //     result[i] = RegistersCollection.Read((byte)regAddress);
            //     this.Log(LogLevel.Error, "Read value {0}", result[i]);
            //     RegistersAutoIncrement();
            // }
            return result;
        }

        // public decimal AccelerationX
        // {
        //     get => accelarationX;
        //     set
        //     {
        //         if (!IsAccelerationOutOfRange(value))
        //         {
        //             accelarationX = value;
        //             this.Log(LogLevel.Noisy, "AccelerationX set to {0}", accelarationX);
        //         }
        //     }
        // }

        // public decimal AccelerationY
        // {
        //     get => accelarationY;
        //     set
        //     {
        //         if (!IsAccelerationOutOfRange(value))
        //         {
        //             accelarationY = value;
        //             this.Log(LogLevel.Noisy, "AccelerationY set to {0}", accelarationY);
        //         }
        //     }
        // }

        // public decimal AccelerationZ
        // {
        //     get => accelarationZ;
        //     set
        //     {
        //         if (!IsAccelerationOutOfRange(value))
        //         {
        //             accelarationZ = value;
        //             this.Log(LogLevel.Noisy, "AccelerationZ set to {0}", accelarationZ);
        //         }
        //     }
        // }

        public decimal Temperature { get; set; }

        public GPIO IRQ { get; }
        public ByteRegisterCollection RegistersCollection { get; }

        private void DefineRegisters()
        {
            // Registers.WhoAmI.Define(this, 0x43);

            // Registers.Control1.Define(this) //RW
            //     .WithTaggedFlag("BLOCK_DATA_UPDATE", 0)
            //     .WithFlag(1, out highFreqDataRateMode, name: "HIGH_FREQ_MODE_ENABLE")
            //     .WithValueField(2, 2, out fullScale, name: "FULL_SCALE_SELECT")
            //     .WithValueField(4, 4, out outDataRate, name: "OUTPUT_DATA_RATE");

            // Registers.Control4.Define(this, 0x01) //RW
            //     .WithFlag(0, out readyEnabled, name: "DATA_READY_IRQ1_ENABLE")
            //     .WithTaggedFlag("FIFO_THRESHOLD_IRQ1_ENABLE", 1)
            //     .WithTaggedFlag("6D_RECON_IRQ1_ENABLE", 2)
            //     .WithTaggedFlag("DOUBLE_TAP_RECON_IRQ1_ENABLE", 3)
            //     .WithTaggedFlag("FREE_FALL_RECON_IRQ1_ENABLE", 4)
            //     .WithTaggedFlag("WAKEUP_RECON_IRQ1_ENABLE", 5)
            //     .WithTaggedFlag("SINGLE_TAP_RECON_IRQ1_ENABLE", 6)
            //     .WithTaggedFlag("MASTER_DATA_READY_IRQ1_ENABLE", 7)
            //     .WithWriteCallback((_, __) => UpdateInterrupts());

            Registers.Temp.Define(this)
                .WithValueField(0, 8, FieldMode.Read, name: "TEMPERATURE_SENSOR", valueProviderCallback: _ => TwoComplementSignConvert(Temperature));

            // Registers.Status.Define(this) //RO
            //     .WithFlag(0, out readyPending, FieldMode.Read, name: "XYZ_DATA_AVAILABLE")
            //     .WithTaggedFlag("FREE_FALL_EVENT_DETECT", 1)
            //     .WithTaggedFlag("CHANGE_IN_POSITION_DETECT", 2)
            //     .WithTaggedFlag("SINGLE_TAP_EVENT_DETECT", 3)
            //     .WithTaggedFlag("DOUBLE_TAP_EVENT_DETECT", 4)
            //     .WithTaggedFlag("SLEEP_EVENT_DETECT", 5)
            //     .WithTaggedFlag("WAKEUP_EVENT_DETECT", 6)
            //     .WithTaggedFlag("FIFO_REACH_THRESHOLD", 7);

            // Registers.DataOutXLow.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "X_ACCEL_DATA[7:2]", valueProviderCallback: _ => Convert(AccelerationX, upperByte: false));

            // Registers.DataOutXHigh.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "X_ACCEL_DATA[15:8]", valueProviderCallback: _ => Convert(AccelerationX, upperByte: true));

            // Registers.DataOutYLow.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "Y_ACCEL_DATA[7:2]", valueProviderCallback: _ => Convert(AccelerationY, upperByte: false));

            // Registers.DataOutYHigh.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "Y_ACCEL_DATA[15:8]", valueProviderCallback: _ => Convert(AccelerationY, upperByte: true));

            // Registers.DataOutZLow.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "Z_ACCEL_DATA[7:2]", valueProviderCallback: _ => Convert(AccelerationZ, upperByte: false));

            // Registers.DataOutZHigh.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "Z_ACCEL_DATA[15:8]", valueProviderCallback: _ => Convert(AccelerationZ, upperByte: true));

            // Registers.StatusEventDetection.Define(this) //RO
            //     .WithFlag(0, out readyPending, FieldMode.Read, name: "XYZ_DATA_AVAILABLE")
            //     .WithTaggedFlag("FREE_FALL_EVENT_DETECT", 1)
            //     .WithTaggedFlag("CHANGE_IN_POSITION_DETECT", 2)
            //     .WithTaggedFlag("SINGLE_TAP_EVENT_DETECT", 3)
            //     .WithTaggedFlag("DOUBLE_TAP_EVENT_DETECT", 4)
            //     .WithTaggedFlag("SLEEP_EVENT_DETECT", 5)
            //     .WithTaggedFlag("WAKEUP_EVENT_DETECT", 6)
            //     .WithTaggedFlag("FIFO_REACH_THRESHOLD", 7);
        }

        private void RegistersAutoIncrement()
        {
            // if(regAddress >= Registers.DataOutXLow && regAddress < Registers.DataOutZHigh)
            // {
            //     regAddress = (Registers)((int)regAddress + 1);
            //     this.Log(LogLevel.Noisy, "Auto-incrementing to the next register 0x{0:X} - {0}", regAddress);
            // }
        }

        private void UpdateInterrupts()
        {
            var status = readyEnabled.Value && readyPending.Value;
            this.Log(LogLevel.Noisy, "Setting IRQ to {0}", status);
            IRQ.Set(status);
        }

        // private decimal GetSenesorSensitivity()
        // {
        //     decimal gain = SensorSensitivity;
        //     switch(fullScale.Value)
        //     {
        //         case (uint)FullScaleSelect.fullScale2g:
        //             gain = SensorSensitivity;
        //             break;
        //         case (uint)FullScaleSelect.fullScale16g:
        //             gain = 8 * SensorSensitivity;
        //             break;
        //         case (uint)FullScaleSelect.fullScale4g:
        //             gain = 2 * SensorSensitivity;
        //             break;
        //         case (uint)FullScaleSelect.fullScale8g:
        //             gain = 4 * SensorSensitivity;
        //             break;
        //         default:
        //             gain = SensorSensitivity;
        //             break;
        //     }
        //     return gain;
        // }

        // private bool IsAccelerationOutOfRange(decimal acceleration)
        // {
        //     // This range protects from the overflow of the short variables in the 'Convert' function.
        //     if (acceleration < MinAcceleration || acceleration > MaxAcceleration)
        //     {
        //         this.Log(LogLevel.Warning, "Acceleration is out of range, use value from the range <-19.5;19.5>");
        //         return true;
        //     }
        //     return false;
        // }

        // private byte Convert(decimal value, bool upperByte)
        // {
        //     byte result = 0;
        //     decimal gain = GetSenesorSensitivity();
        //     value = (value * 1000 / gain) / GravitationalConst;
        //     var valueAsShort = (short)value;

        //     if(upperByte)
        //     {
        //         result = (byte)(valueAsShort >> 8);
        //     }
        //     else
        //     {
        //         if(highFreqDataRateMode.Value &&
        //             outDataRate.Value >= (byte)DataRateModeStartRange.HighFreqDataRateStartRange &&
        //             outDataRate.Value < (byte)DataRateModeStartRange.LowPowerDataRateStartRange)
        //         {
        //             result = (byte)(valueAsShort & (byte)CoverBytes.HighFreqMode);
        //             this.Log(LogLevel.Noisy, "High frequencies mode is selected.");
        //         }
        //         else if(outDataRate.Value >= (byte)DataRateModeStartRange.LowPowerDataRateStartRange)
        //         {
        //             result = (byte)(valueAsShort & (byte)CoverBytes.LowPowerMode);
        //             this.Log(LogLevel.Noisy, "Low power mode is selected.");
        //         }
        //         else
        //         {
        //             result = (byte)(valueAsShort & (byte)CoverBytes.NoneExtraModes);
        //             this.Log(LogLevel.Noisy, "High frequencies and low power modes aren't selected.");
        //         }
        //     }
        //     return result;
        // }

        private byte TwoComplementSignConvert(decimal temp)
        {
            byte tempAsByte = Decimal.ToByte(temp);
            if(temp < 0)
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
        private Registers regAddress;

        private ushort controlStatus;
        private ushort response;

        private enum Registers : byte
        {
            Cntl = 0x00,
            // Reserved: 0x01
            Temp = 0x02,
            // Reserved: 0x03
            Volt = 0x04,
            // Reserved: 0x05
            Flag = 0x06,
            // Reserved: 0x07
            NomCap = 0x08,
            // Reserved: 0x09
            FullCap = 0x0A,
            // Reserved: 0x0B
            Rm = 0x0C,
            // Reserved: 0x0d
            Fcc = 0x0e,
            // Reserved: 0x0f
            AvgCur = 0x10,
            // Reserved: 0x11
            SbyCur = 0x12,
            // Reserved: 0x13
            MaxCur = 0x14,
            // Reserved: 0x15 - 0x17
            AvgPwr = 0x18,
            // Reserved: 0x19 = 0x1b
            Soc = 0x1c,
            // Reserved: 0x1d
            IntTemp = 0x1e,
            // Reserved: 0x1f
            Soh = 0x20,
        }
    }
}
