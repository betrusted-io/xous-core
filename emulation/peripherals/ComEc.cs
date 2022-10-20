//
// Copyright (c) 2010-2021 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Core.Structure.Registers;
using System.Collections.Concurrent;

namespace Antmicro.Renode.Peripherals.SPI.Betrusted
{
    public class BetrustedEcRam : IDoubleWordPeripheral, IKnownSize
    {
        public BetrustedEcRam(Machine machine, BetrustedEcCom spi, long size)
        {
            this.machine = machine;
            this.spi = spi;
            this.size = size;

            // Hardware defaults to 1280 entries
            readFifo = new ConcurrentQueue<uint>();
            writeFifo = new ConcurrentQueue<uint>();
        }

        private Machine machine;
        private BetrustedEcCom spi;
        private long size;
        private ConcurrentQueue<uint> readFifo;
        private ConcurrentQueue<uint> writeFifo;
        public long Size { get { return size; } }

        public bool WriteNextWord(uint value)
        {
            readFifo.Enqueue(value);
            return true;
        }

        public bool ReadFifoHasData()
        {
            return !readFifo.IsEmpty;
        }

        public int ReadFifoCount()
        {
            return readFifo.Count;
        }

        public bool GetNextWord(out uint value)
        {
            bool have_data = writeFifo.TryDequeue(out value);
            return have_data;
        }

        public int WriteFifoCount()
        {
            return writeFifo.Count;
        }

        public bool WriteFifoHasData()
        {
            return !writeFifo.IsEmpty;
        }

        public void WriteDoubleWord(long address, uint value)
        {
            if (address == 0)
            {
                // machine.Log(LogLevel.Error, "EC queueing word: {0:X4}", value);
                writeFifo.Enqueue(value);
                spi.Hold.Set(false);
                return;
            }
            machine.Log(LogLevel.Error, "Write to unsupported address: {}", address);
        }

        public uint ReadDoubleWord(long address)
        {
            uint ret = 0;
            if ((address == 0) && readFifo.TryDequeue(out ret))
            {
                // machine.Log(LogLevel.Error, "EC received word: {0:X4}", ret);
            }
            else
            {
                // machine.Log(LogLevel.Error, "Read double word, but have no data");
            }
            return ret;
        }

        public void Reset()
        {
            readFifo = new ConcurrentQueue<uint>();
            writeFifo = new ConcurrentQueue<uint>();
        }
    }

    public class BetrustedEcCom : IComPeripheral, IDoubleWordPeripheral, IKnownSize, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IGPIOSender
    {

        public BetrustedEcCom(Machine machine, uint memAddr, uint memSize)
        {
            Hold = new GPIO();
            Interrupt = new GPIO();
            EcRam = new BetrustedEcRam(machine, this, memSize);
            machine.SystemBus.Register(EcRam, new BusRangeRegistration(memAddr, memSize));

            RegistersCollection = new DoubleWordRegisterCollection(this);
            Registers.ComControl.Define32(this)
                .WithFlag(0, FieldMode.Write, name: "CLRERR")
                .WithFlag(1, FieldMode.Write, writeCallback: (_, val) => Interrupt.Set(val), name: "HOST_INT")
                .WithFlag(2, FieldMode.Write, writeCallback: (_, val) =>
                {
                    if (!val)
                        return;
                    machine.Log(LogLevel.Debug, "Resetting FIFO due to RESET bit being set");
                    Reset();
                }, name: "RESET")
            ;

            Registers.ComStatus.Define32(this)
                .WithFlag(0, FieldMode.Read, valueProviderCallback: (_) => false, name: "TIP")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: (_) => EcRam.ReadFifoHasData(), name: "RX_AVAIL")
                .WithFlag(2, FieldMode.Read, valueProviderCallback: (_) => EcRam.ReadFifoCount() >= 1280, name: "RX_OVER")
                .WithFlag(3, FieldMode.Read, valueProviderCallback: (_) => EcRam.ReadFifoCount() == 0, name: "RX_UNDER")
                .WithValueField(4, 12, FieldMode.Read, valueProviderCallback: (_) => (uint)EcRam.ReadFifoCount(), name: "RX_LEVEL")
                .WithFlag(16, FieldMode.Read, valueProviderCallback: (_) => EcRam.WriteFifoHasData(), name: "TX_AVAIL")
                .WithFlag(17, FieldMode.Read, valueProviderCallback: (_) => EcRam.WriteFifoCount() == 0, name: "TX_EMPTY")
                .WithValueField(18, 12, FieldMode.Read, valueProviderCallback: (_) => (uint)EcRam.WriteFifoCount(), name: "TX_LEVEL")
                .WithFlag(30, FieldMode.Read, valueProviderCallback: (_) => EcRam.WriteFifoCount() >= 1280, name: "TX_OVER")
                .WithFlag(31, FieldMode.Read, valueProviderCallback: (_) => false, name: "TX_UNDER")
            ;
            Reset();
        }

        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public ushort Transmit(ushort value)
        {
            uint TxValue;
            if (!EcRam.GetNextWord(out TxValue))
            {
                TxValue = 0xDDDD;
            }
            // this.Log(LogLevel.Noisy, "EC received:{0:X4} sent:{1:X4}", value, TxValue);
            Hold.Set(!EcRam.WriteFifoHasData());
            EcRam.WriteNextWord((ushort)value);
            // this.Log(LogLevel.Info, "EcRam.ReadFifo now contains {0} items, EcRam.WriteFifo contains {1} items", EcRam.ReadFifoCount(), EcRam.WriteFifoCount());
            return (ushort)(TxValue & 0xffff);
        }

        public void FinishTransmission()
        {
            // this.Log(LogLevel.Info, "EC finished transmission");
        }

        public bool HasData()
        {
            return EcRam.WriteFifoHasData();
        }

        public void Reset()
        {
            EcRam.Reset();
            Hold.Set(true);
            Interrupt.Set(false);
        }

        public long Size { get { return 0x100; } }
        private uint TxValue;
        private BetrustedEcRam EcRam;
        public GPIO Hold { get; }
        public GPIO Interrupt { get; }
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        private enum Registers
        {
            ComControl = 0x00,

            ComStatus = 0x04,
        }
    }
}
