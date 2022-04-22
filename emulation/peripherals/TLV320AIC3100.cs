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
    public class TLV320AIC3100 : II2CPeripheral, IProvidesRegisterCollection<ByteRegisterCollection>
    {
        public TLV320AIC3100()
        {
            RegistersCollection = new ByteRegisterCollection(this);
            GPIO1 = new GPIO();
            DOUT = new GPIO();
            DefineRegisters();
            Reset();
        }

        private bool firstByte;
        private bool secondByte;

        public void FinishTransmission()
        {
        }

        public void Reset()
        {
            RegistersCollection.Reset();
            GPIO1.Set(false);
            DOUT.Set(false);
            regAddress = 0;
            currentPage = 0;
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
                regAddress = (long)data[0];
                return;
            }
            uint dataOffset = 1;

            // Treat register address 0 as special, since it's the universal "page selector" register
            if (regAddress == 0 && dataOffset < data.Length) {
                RegistersCollection.Write((long)regAddress, data[dataOffset]);
                regAddress += 1;
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
        }

        public byte[] Read(int count)
        {
            this.Log(LogLevel.Noisy, "Reading {0} bytes beginning at register {1} (0x{1:X})", count, (Registers)regAddress);
            var result = new byte[count];

            if (currentPage == 0) {
                for (var i = 0; i < count; i++)
                {
                    result[i] = RegistersCollection.Read(regAddress + i);
                }
                regAddress += count;
            }
            return result;
        }

        public GPIO GPIO1 { get; }
        public GPIO DOUT { get; }
        public ByteRegisterCollection RegistersCollection { get; }

        private void DefineRegisters()
        {
            // Registers.TempLow.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "TEMPERATURE_SENSOR_LOW", valueProviderCallback: _ => ((uint)(Temperature * 10)));
            // Registers.TempHigh.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "TEMPERATURE_SENSOR_LOW", valueProviderCallback: _ => ((uint)(Temperature * 10)) >> 8);
            // Registers.VoltLow.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "VOLTAGE_LOW", valueProviderCallback: _ => Voltage);
            // Registers.VoltHigh.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "VOLTAGE_HIGH", valueProviderCallback: _ => Voltage >> 8);
            // Registers.AvgCurLow.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "AVGCUR_LOW", valueProviderCallback: _ => (uint)AvgCur);
            // Registers.AvgCurHigh.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "AVGCUR_HIGH", valueProviderCallback: _ => (uint)(AvgCur >> 8));
            // Registers.RmLow.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "RM_LOW", valueProviderCallback: _ => RemainingCapacity);
            // Registers.RmHigh.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "RM_HIGH", valueProviderCallback: _ => RemainingCapacity >> 8);
            // Registers.SocLow.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "SOC_LOW", valueProviderCallback: _ => SoC);
            // Registers.SocHigh.Define(this)
            //     .WithValueField(0, 8, FieldMode.Read, name: "SOC_HIGH", valueProviderCallback: _ => SoC >> 8);
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

        private IFlagRegisterField readyPending;
        private IFlagRegisterField readyEnabled;
        private long regAddress;

        private byte currentPage;

        private enum Registers : byte
        {
            PageCtrl = 0,
            SoftwareReset = 1,
            // Reserved: 2
            OtFlag = 3,
            ClockGenMuxing = 4,
            PllPnR = 5,
            PllJ = 6,
            PllDHigh = 7,
            PllDLow = 8,
            // Reserved: 9
            // Reserved: 10
            DacNdac = 11,
            DacMdac = 12,
            DacDosrHigh = 13,
            DacDosrLow = 14,
            DacIdac = 15,
            DacPrbEngine = 16,
            // Reserved = 17,
            AdcNadc = 18,
            AdcMadc = 19,
            AdcAosr = 20,
            AdcIadc = 21,
            AdcPrbDec = 22,
            // Reserved = 23,
            // Reserved = 24,
            ClkOutMux = 25,
            ClkOutM = 26,
            CodecInterface = 27,
            DataSlotOffset = 28,
            CodecInterface2 = 29,
            BclkN = 30,
            CodecSecondaryInterfaceControl1 = 31,
            CodecSecondaryInterfaceControl2 = 32,
            CodecSecondaryInterfaceControl3 = 33,
            I2cBusCondition = 34,
            // Reserved = 35,
            AdcFlag = 36,
            DacFlag = 37,
            DacFlag2 = 38,
            OverflowFlags = 39,
            // Reserved = 40,
            // Reserved = 41,
            // Reserved = 42,
            // Reserved = 43,
            DacInterruptFlags = 44,
            AdcInterruptFlags = 45,
            DacInterruptFlags2 = 46,
            AdcInterruptFlags2 = 47,
            Int1Control = 48,
            Int2Control = 49,
            // Reserved = 50,
            Gpio1InOutPinControl = 51,
            // Reserved = 52,
            DoutPinControl = 53,
            DinPinControl = 54,
            // Reserved = 55,
            // Reserved = 56,
            // Reserved = 57,
            // Reserved = 58,
            // Reserved = 59,
            DacInstructionSet = 60,
            AdcInstructionSet = 61,
            InstructionModeControlBits = 62,
            DacDataPathSetup = 63,
            DacVolumeControl = 64,
            DacLeftVolumeControl = 65,
            DacRightVolumeControl = 66,
            HeadsetDetection = 67,
            DrcControl1 = 68,
            DrcControl2 = 69,
            DrcControl3 = 70,
            LeftBeepGenerator = 71,
            RightBeepGenerator = 72,
            BeepLengthHigh = 73,
            BeepLengthMid = 74,
            BeepLengthLow = 75,
            BeepSinxHigh = 76,
            BeepSinxLow = 77,
            BeepCosxHigh = 78,
            BeepCosxLow = 79,
            // Reserved = 80,
            AdcDigitalMic = 81,
            AdcVolumeControlFine = 82,
            AdcVolumeControlCoarse = 83,
            // Reserved = 84,
            // Reserved = 85,
            AgcControl1 = 86,
            AgcControl2 = 87,
            AgcMaximum = 88,
            AgcAttack = 89,
            AgcDecay = 90,
            AgcNoiseDebounce = 91,
            AgcSignalDebounce = 92,
            AgcGainAppliedReading = 93,
            // Reserved = 94,
            // Reserved = 95,
            // Reserved = 96,
            // Reserved = 97,
            // Reserved = 98,
            // Reserved = 99,
            // Reserved = 100,
            // Reserved = 101,
            AdcDcMeasurement1 = 102,
            AdcDcMeasurement2 = 103,
            AdcDcMeasurementOutput1 = 104,
            AdcDcMeasurementOutput2 = 105,
            AdcDcMeasurementOutput3 = 106,
            // Reserved = 107,
            // Reserved = 108,
            // Reserved = 109,
            // Reserved = 110,
            // Reserved = 111,
            // Reserved = 112,
            // Reserved = 113,
            // Reserved = 114,
            // Reserved = 115,
            VolMicDetPinSar = 116,
            VolMicDetPinGain = 117,
            // Reserved = 118,
            // Reserved = 119,
            // Reserved = 120,
            // Reserved = 121,
            // Reserved = 122,
            // Reserved = 123,
            // Reserved = 124,
            // Reserved = 125,
            // Reserved = 126,
            // Reserved = 127,
        }
    }
}
