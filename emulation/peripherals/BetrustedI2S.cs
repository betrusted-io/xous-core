//
// Copyright (c) 2010-2020 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//

using System;
using System.Linq;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Sound;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.Sound.Betrusted
{
    public class AudioRam : IBytePeripheral, IDoubleWordPeripheral, IWordPeripheral, IKnownSize
    {
        public AudioRam(Machine machine, BetrustedI2S audio, long size)
        {
            this.machine = machine;
            this.audio = audio;
            this.size = size;
            this.data = new byte[size];
        }

        private Machine machine;
        private BetrustedI2S audio;
        private long size;
        public byte[] data;
        public long Size { get { return size; } }

        public void WriteDoubleWord(long address, uint value)
        {
            var bytes = BitConverter.GetBytes(value);
            var i = 0;
            for (i = 0; i < bytes.Length; i++)
            {
                this.data[address + i] = bytes[i];
            }
        }

        public uint ReadDoubleWord(long offset)
        {
            return (((uint)this.data[offset]) << 0) | (((uint)this.data[offset + 1]) << 8) | (((uint)this.data[offset + 2]) << 16) | ((uint)(this.data[offset + 3]) << 24);
        }

        public void WriteWord(long address, ushort value)
        {
            var bytes = BitConverter.GetBytes(value);
            var i = 0;
            for (i = 0; i < bytes.Length; i++)
            {
                this.data[address + i] = bytes[i];
            }
        }

        public ushort ReadWord(long address)
        {
            return (ushort)((((ushort)this.data[address]) << 0) | (((ushort)this.data[address + 1]) << 8));
        }

        public byte ReadByte(long offset)
        {
            return this.data[offset];
        }

        public void WriteByte(long offset, byte value)
        {
            this.data[offset] = value;
        }

        public void Reset()
        {
            for (var i = 0; i < this.data.Length; i++)
            {
                this.data[i] = 0;
            }
        }
    }

    public class BetrustedI2S : BasicDoubleWordPeripheral, IDisposable, IKnownSize
    {
        public BetrustedI2S(Machine machine, uint memAddr, uint memSize) : base(machine)
        {
            this.audioRam = new AudioRam(machine, this, memSize);
            this.bufferAddress = memAddr;
            machine.SystemBus.Register(this.audioRam, new BusRangeRegistration(memAddr, memSize));

            DefineRegisters();
            IRQ = new GPIO();
            Reset();
        }

        public void Dispose()
        {
            encoder?.Dispose();
        }

        public override void Reset()
        {
            base.Reset();
            IRQ.Unset();
            decoder?.Reset();
            encoder?.FlushBuffer();

            sampleRatio = 256;
            sampleWidth = 8;
            numberOfChannels = 2;
            masterFrequency  = 4000000;
            samplesPerDoubleWord = 4;
        }

        public GPIO IRQ { get; }
        public string InputFile  { get; set; }
        public string OutputFile  { get; set; }
        public long Size => 0x1000;

        private void UpdateInterrupts()
        {
            var rxReady = eventRxReady.Value && interruptEnableRxReady.Value;
            var rxError = eventRxError.Value && interruptEnableRxError.Value;
            var txReady = eventTxReady.Value && interruptEnableTxReady.Value;
            var txError = eventTxError.Value && interruptEnableTxError.Value;
            IRQ.Set(rxReady || rxError || txReady || txError);
        }

        private void StartTx()
        {
            if(enableTx.Value)
            {
                if(OutputFile == "")
                {
                    this.Log(LogLevel.Error, "Starting transmission without an output file!");
                    return;
                }
                encoder = new PCMEncoder(sampleWidth, sampleFrequency, numberOfChannels, false);
                encoder.SetBufferingBySamplesCount(maxSamplesCount.Value);
                encoder.Output = OutputFile;
            }
            StartTxThread();
        }

        private void StartRx()
        {
            if(enableRx.Value)
            {
                if(InputFile == "")
                {
                    this.Log(LogLevel.Error, "Starting reception without an input file!");
                    return;
                }
                decoder = new PCMDecoder(sampleWidth, sampleFrequency, numberOfChannels, false, this);
                decoder.LoadFile(InputFile);
            }
            StartRxThread();
        }

        private void Stop()
        {
            encoder?.FlushBuffer();
            StopTxThread();
            StopRxThread();
            // eventStopped.Value = true;
            UpdateInterrupts();
        }

        private void StartTxThread()
        {
            txThread = machine.ObtainManagedThread(OutputFrames, (int)(sampleFrequency / (maxSamplesCount.Value * samplesPerDoubleWord)));
            txThread.Start();
        }

        private void StartRxThread()
        {
            rxThread = machine.ObtainManagedThread(InputFrames, (int)(sampleFrequency / (maxSamplesCount.Value * samplesPerDoubleWord)));
            rxThread.Start();
        }

        private void StopTxThread()
        {
            if(txThread == null)
            {
                this.Log(LogLevel.Debug, "Trying to stop sampling when it is not active");
                return;
            }
            txThread.Stop();
            txThread = null;
        }

        private void StopRxThread()
        {
            if(rxThread == null)
            {
                this.Log(LogLevel.Debug, "Trying to stop sampling when it is not active");
                return;
            }
            rxThread.Stop();
            rxThread = null;
        }


        private void OutputFrames()
        {
            var currentPointer = txdPointer.Value;
            // The TXD.PTR register has been copied to internal double-buffers
            // eventTxPointerUpdated.Value = true;
            UpdateInterrupts();

            // RxTxMaxCnt denotes number of DoubleWords, we need to calculate samples number
            for(var samples = 0u; samples < writeCount.Value * samplesPerDoubleWord; samples++)
            {
                var thisSample = machine.SystemBus.ReadDoubleWord(currentPointer + samples * sampleWidth / 8);
                BitHelper.ClearBits(ref thisSample, (int)sampleWidth, (int)(32 - sampleWidth));
                encoder.AcceptSample(thisSample);
            }
        }

        private void InputFrames()
        {
            var currentPointer = rxdPointer.Value;
            // The RXD.PTR register has been copied to internal double-buffers
            // eventRxPointerUpdated.Value = true;
            UpdateInterrupts();

            for(var doubleWords = 0u; doubleWords < readCount.Value; doubleWords++)
            {
                // Double word may consist on many samples when sampleWidth is not equal 32bit
                uint valueToStore = 0;
                for(var sampleOffset = samplesPerDoubleWord; sampleOffset > 0; sampleOffset--)
                {
                    valueToStore |= decoder.GetSingleSample() << (int)(sampleWidth * (sampleOffset - 1));
                }
                machine.SystemBus.WriteDoubleWord(currentPointer + doubleWords * 4, valueToStore);
            }
        }
        
        private void SetMasterClockLrckRatio(uint value)
        {
            switch((MasterLrClockRatio)value)
            {
                case MasterLrClockRatio.X32:
                    sampleRatio = 32;
                    break;
                case MasterLrClockRatio.X48:
                    sampleRatio = 48;
                    break;
                case MasterLrClockRatio.X64:
                    sampleRatio = 64;
                    break;
                case MasterLrClockRatio.X96:
                    sampleRatio = 96;
                    break;
                case MasterLrClockRatio.X128:
                    sampleRatio = 128;
                    break;
                case MasterLrClockRatio.X192:
                    sampleRatio = 192;
                    break;
                case MasterLrClockRatio.X256:
                    sampleRatio = 256;
                    break;
                case MasterLrClockRatio.X384:
                    sampleRatio = 384;
                    break;
                case MasterLrClockRatio.X512:
                    sampleRatio = 512;
                    break;
                default:
                    this.Log(LogLevel.Error, "Wrong CONFIG.RATIO value");
                    break;
            }
            SetSampleFrequency();
        }

        private void SetMasterClockFrequency(uint val)
        {
            switch((MasterClockFrequency)val)
            {
                case MasterClockFrequency.Mhz32Div8:
                    masterFrequency = 32000000 / 8;
                    break;
                case MasterClockFrequency.Mhz32Div10:
                    masterFrequency = 32000000 / 10;
                    break;
                case MasterClockFrequency.Mhz32Div11:
                    masterFrequency = 32000000 / 11;
                    break;
                case MasterClockFrequency.Mhz32Div15:
                    masterFrequency = 32000000 / 15;
                    break;
                case MasterClockFrequency.Mhz32Div16:
                    masterFrequency = 32000000 / 16;
                    break;
                case MasterClockFrequency.Mhz32Div21:
                    masterFrequency = 32000000 / 21;
                    break;
                case MasterClockFrequency.Mhz32Div23:
                    masterFrequency = 32000000 / 23;
                    break;
                case MasterClockFrequency.Mhz32Div30:
                    masterFrequency = 32000000 / 30;
                    break;
                case MasterClockFrequency.Mhz32Div31:
                    masterFrequency = 32000000 / 31;
                    break;
                case MasterClockFrequency.Mhz32Div32:
                    masterFrequency = 32000000 / 32;
                    break;
                case MasterClockFrequency.Mhz32Div42:
                    masterFrequency = 32000000 / 42;
                    break;
                case MasterClockFrequency.Mhz32Div63:
                    masterFrequency = 32000000 / 63;
                    break;
                case MasterClockFrequency.Mhz32Div125:
                    masterFrequency = 32000000 / 125;
                    break;
                default:
                    this.Log(LogLevel.Error, "Wrong CONFIG.MCK value");
                    break;
            }
            SetSampleFrequency();
        }

        private void SetSampleWidth(uint value)
        {
            // Only 3 values possible:
            //  0  -  8  Bit
            //  1  -  16 Bit (Default)
            //  2  -  32 Bit
            if(value > 2)
            {
                this.Log(LogLevel.Warning, "Sample width set to invalid value : 0x{0:X}. Setting default value.", value);
                value = 1;
            }
            sampleWidth = (uint)(8 * (1 << (int)value));
            samplesPerDoubleWord = 32 / sampleWidth;
            SetSampleFrequency();
        }

        private void SetSampleFrequency()
        {
            if(sampleRatio < 2 * sampleWidth)
            {
                this.Log(LogLevel.Error, "Invalid CONFIG.RATIO value, it cannot exceed `2* CONFIG.SWIDTH`");
            }
            sampleFrequency = GetClosestValue(masterFrequency / sampleRatio, possibleSamplingRates);
            this.Log(LogLevel.Debug, "Set sample frequency to {0}Hz, {1}Bit", sampleFrequency, sampleWidth);
        }

        private uint GetClosestValue(uint freq, uint[] possibleVals)
        {
            var closest = possibleVals.OrderBy(x => Math.Abs((long) x - freq)).First();
            return closest;
        }

        private void DefineRegisters()
        {
            Registers.EventEnable.Define(this)
                .WithFlag(0, out interruptEnableRxReady, changeCallback: (_, __) => UpdateInterrupts(), name: "RX_READY")
                .WithFlag(1, out interruptEnableRxError, changeCallback: (_, __) => UpdateInterrupts(), name: "RX_ERROR")
                .WithFlag(2, out interruptEnableTxReady, changeCallback: (_, __) => UpdateInterrupts(), name: "TX_READY")
                .WithFlag(3, out interruptEnableTxError, changeCallback: (_, __) => UpdateInterrupts(), name: "TX_ERROR")
            ;
            Registers.EventStatus.Define(this)
                .WithFlag(0, FieldMode.Read, valueProviderCallback: (_) => false, changeCallback: (_, __) => UpdateInterrupts(), name: "RX_READY")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: (_) => false, changeCallback: (_, __) => UpdateInterrupts(), name: "RX_ERROR")
                .WithFlag(2, FieldMode.Read, valueProviderCallback: (_) => false, changeCallback: (_, __) => UpdateInterrupts(), name: "TX_READY")
                .WithFlag(3, FieldMode.Read, valueProviderCallback: (_) => false, changeCallback: (_, __) => UpdateInterrupts(), name: "TX_ERROR")
            ;
            Registers.EventPending.Define(this)
                .WithFlag(0, out eventRxReady, FieldMode.WriteOneToClear, name: "RX_READY")
                .WithFlag(1, out eventRxError, FieldMode.WriteOneToClear, name: "RX_ERROR")
                .WithFlag(2, out eventTxReady, FieldMode.WriteOneToClear, name: "TX_READY")
                .WithFlag(3, out eventTxError, FieldMode.WriteOneToClear, name: "TX_ERROR")
            ;
            Registers.TxCtl.Define32(this)
                .WithFlag(0, writeCallback: (_, val) => { if (val) StartTx(); }, name: "tx_enable")
            ;

            Registers.RxCtl.Define32(this)
                .WithFlag(0, writeCallback: (_, val) => { if (val) StartRx(); }, name: "rx_enable")
            ;
        }

        private IFlagRegisterField enableI2S;
        private IFlagRegisterField enableRx;
        private IFlagRegisterField enableTx;

        private IFlagRegisterField eventRxReady;
        private IFlagRegisterField eventRxError;
        private IFlagRegisterField eventTxReady;
        private IFlagRegisterField eventTxError;
        private IFlagRegisterField interruptEnableRxReady;
        private IFlagRegisterField interruptEnableRxError;
        private IFlagRegisterField interruptEnableTxReady;
        private IFlagRegisterField interruptEnableTxError;

        private IValueRegisterField maxSamplesCount;
        private IValueRegisterField rxdPointer;
        private IValueRegisterField txdPointer;

        private uint masterFrequency;
        private uint numberOfChannels;
        private uint sampleFrequency;
        private uint sampleRatio;
        private uint samplesPerDoubleWord;
        private uint sampleWidth;

        private IManagedThread rxThread;
        private IManagedThread txThread;
        private PCMDecoder decoder;
        private PCMEncoder encoder;
        private AudioRam audioRam;
        private uint bufferAddress;

        private enum DataFormat
        {
            Standard      = 0,
            LeftJustified = 1,
        }
        private enum Registers :long
        {
            EventStatus = 0x00,
            EventPending = 0x04,
            EventEnable = 0x08,
            RxCtl = 0x0c,
            RxStat = 0x10,
            RxConf = 0x14,
            TxCtl = 0x18,
            TxStat = 0x1c,
            TxConf = 0x20,
        }
    }
}
