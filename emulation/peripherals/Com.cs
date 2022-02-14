//
// Copyright (c) 2010-2021 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Exceptions;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Time;
using Antmicro.Renode.Utilities;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Core.Structure.Registers;
using System.Collections.Concurrent;

namespace Antmicro.Renode.Peripherals.SPI
{
    public interface IComPeripheral : IPeripheral, IInterestingType
    {
        ushort Transmit(ushort data);
        void FinishTransmission();
    }
    public interface IComController : IPeripheral, IInterestingType, IGPIOReceiver
    {
        ushort Transmit(ushort data);
        void FinishTransmission();
    }
    public static class ComConnectorExtensions
    {
        public static void CreateComConnector(this Emulation emulation, string name)
        {
            emulation.ExternalsManager.AddExternal(new BetrustedComConnector(), name);
        }
    }

    // This class is responsible for synchronizing communication between the EC and SoC.
    // Note that the "Controller" is the SoC side and the "Peripheral" is the EC side.
    public class BetrustedComConnector : IExternal, IComController, IConnectable<IComPeripheral>, IConnectable<NullRegistrationPointPeripheralContainer<IComController>>, IGPIOReceiver
    {
        public BetrustedComConnector()
        {
        }

        public void Reset()
        {
        }

        public void AttachTo(NullRegistrationPointPeripheralContainer<IComController> controller)
        {
            lock (locker)
            {
                if (controller == this.controller)
                {
                    throw new RecoverableException("Cannot attach to the provided peripheral as it is already registered in this connector.");
                }
                else if (this.controller != null)
                {
                    this.Log(LogLevel.Warning, "Overwriting controller connection.");
                }

                this.controller?.Unregister(this);
                this.controller = controller;
                this.controller.Register(this, NullRegistrationPoint.Instance);
                foreach (var gpio in controller.GetGPIOs())
                {
                    if (gpio.Item1 == "HOLD")
                    {
                        this.holdGpio = gpio.Item2;
                    }
                    if (gpio.Item1 == "INTERRUPT")
                    {
                        this.interruptGpio = gpio.Item2;
                    }
                }
            }
        }

        public void AttachTo(IComPeripheral peripheral)
        {
            lock (locker)
            {
                if (peripheral == this.peripheral)
                {
                    throw new RecoverableException("Cannot attach to the provided peripheral as it is already registered in this connector.");
                }
                else if (this.peripheral != null)
                {
                    this.Log(LogLevel.Warning, "Overwriting peripheral connection.");
                }
                this.peripheral = peripheral;
                foreach (var gpio in peripheral.GetGPIOs())
                {
                    if (gpio.Item1 == "Hold")
                    {
                        gpio.Item2.Connect(this, 0);
                    }
                    else if (gpio.Item1 == "Interrupt")
                    {
                        gpio.Item2.Connect(this, 1);
                    }
                }
            }
        }

        public void DetachFrom(NullRegistrationPointPeripheralContainer<IComController> controller)
        {
            lock (locker)
            {
                if (controller == this.controller)
                {
                    this.controller.Unregister(this);
                    this.controller = null;
                }
                else
                {
                    throw new RecoverableException("Cannot detach from the provided controller as it is not registered in this connector.");
                }
            }
        }

        public void DetachFrom(IComPeripheral peripheral)
        {
            lock (locker)
            {
                if (peripheral == this.peripheral)
                {
                    foreach (var gpio in peripheral.GetGPIOs())
                    {
                        if (gpio.Item1 == "Hold")
                        {
                            gpio.Item2.Disconnect();
                        }
                        else if (gpio.Item1 == "Interrupt")
                        {
                            gpio.Item2.Disconnect();
                        }
                    }
                    peripheral = null;
                }
                else
                {
                    throw new RecoverableException("Cannot detach from the provided peripheral as it is not registered in this connector.");
                }
            }
        }

        public void OnGPIO(int number, bool value)
        {
            if (number == 0)
            {
                // "Hold" pin
                if (holdGpio != null)
                {
                    // controller.GetMachine().HandleTimeDomainEvent(holdGpio.Set, value, new TimeStamp(default(TimeInterval), EmulationManager.ExternalWorld));
                    controller.GetMachine().HandleTimeDomainEvent(holdGpio.Set, value, false);
                }
            }
            else if (number == 1)
            {
                // "Interrupt" pin
                if (interruptGpio != null)
                {
                    // controller.GetMachine().HandleTimeDomainEvent(holdGpio.Set, value, new TimeStamp(default(TimeInterval), EmulationManager.ExternalWorld));
                    controller.GetMachine().HandleTimeDomainEvent(holdGpio.Set, value, false);
                }
            }
        }

        public ushort Transmit(ushort data)
        {
            lock (locker)
            {
                if (peripheral == null)
                {
                    this.Log(LogLevel.Warning, "Controller sent data (0x{0:X}), but peripheral is not connected.", data);
                    return 0xDDDD;
                }
                // We don't use a separate time domain here because the peripheral's output is buffered and we
                // want it to operate at the same speed as the host. If we used a separate time domain then there would
                // be gaps in the buffer, whereas the real hardware will have no gaps.
                return peripheral.Transmit(data);
            }
        }

        public void FinishTransmission()
        {
            lock (locker)
            {
                peripheral.GetMachine().HandleTimeDomainEvent<object>(_ => peripheral.FinishTransmission(), null, TimeDomainsManager.Instance.VirtualTimeStamp);
            }
        }

        private NullRegistrationPointPeripheralContainer<IComController> controller;
        private IComPeripheral peripheral;
        private IGPIO holdGpio;
        private IGPIO interruptGpio;
        private readonly object locker = new object();
    }

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
            this.Log(LogLevel.Debug, "EC received:{0:X4} sent:{1:X4}", value, TxValue);
            Hold.Set(!EcRam.WriteFifoHasData());
            EcRam.WriteNextWord((ushort)value);
            // this.Log(LogLevel.Info, "EcRam.ReadFifo now contains {0} items, EcRam.WriteFifo contains {1} items", EcRam.ReadFifoCount(), EcRam.WriteFifoCount());
            return (ushort)(TxValue & 0xffff);
        }

        public void FinishTransmission()
        {
            // this.Log(LogLevel.Info, "EC finished transmission");
        }

        public void Reset()
        {
            EcRam.Reset();
            Hold.Set(true);
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

    public class BetrustedSocCom : NullRegistrationPointPeripheralContainer<IComController>, IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public BetrustedSocCom(Machine machine) : base(machine)
        {
            IRQ = new GPIO();
            HOLD = new GPIO();
            RegistersCollection = new DoubleWordRegisterCollection(this);

            Registers.ComTx.Define32(this)
                .WithValueField(0, 16, FieldMode.Write, writeCallback: (_, val) =>
                {
                    // Skip transaction if HOLD is set
                    // if (AutoHold.Value && (HOLD && HOLD.IsSet)
                    // {
                    //     return;
                    // }
                    Rx.Value = (uint)RegisteredPeripheral.Transmit((ushort)val);
                    RegisteredPeripheral.FinishTransmission();
                    machine.Log(LogLevel.Debug, "SoC sent {0:X4} received {1:X4}", val, Rx.Value);
                    if (IntEna.Value)
                    {
                        SpiIntStatus.Value = true;
                        UpdateInterrupts();
                        SpiIntStatus.Value = false;
                    }
                }, name: "TX")
            ;

            Registers.ComRx.Define32(this)
                .WithValueField(0, 16, out Rx, FieldMode.Read, name: "RX")
            ;

            Registers.ComControl.Define32(this)
                .WithFlag(0, out IntEna, FieldMode.Read | FieldMode.Write, name: "IntEna")
                .WithFlag(1, out AutoHold, FieldMode.Read | FieldMode.Write, name: "AutoHold")
            ;
            Registers.ComStatus.Define32(this)
                .WithFlag(0, out Tip, FieldMode.Read, name: "Tip")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: (_) => HOLD.IsSet, name: "Hold")
            ;

            Registers.ComEvStatus.Define32(this)
                .WithFlag(0, out SpiIntStatus, FieldMode.Read, name: "SpiInt")
                .WithFlag(1, FieldMode.Read, valueProviderCallback: (_) => HOLD.IsSet, name: "SpiHold")
            ;

            Registers.ComEvPending.Define32(this)
                .WithFlag(0, out SpiIntPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "SpiInt", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out SpiHoldPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "SpiHold", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.ComEvEnable.Define32(this)
                .WithFlag(0, out SpiIntEnable, name: "SpiInt", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out SpiHoldEnable, name: "SpiHold", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Reset();
        }

        private void UpdateInterrupts()
        {
            // if (RegisteredPeripheral.Interrupt())
            // {
            //     SpiIntPending.Value = true;
            // }
            if (HOLD.IsSet)
            {
                SpiHoldPending.Value = true;
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
            // IntEna.Value = false;
            // AutoHold.Value = false;
            // Tip.Value = false;
            // Rx.Value = 0;

            SpiIntStatus.Value = false;
            SpiIntPending.Value = false;
            SpiHoldPending.Value = false;
            SpiIntEnable.Value = false;
            SpiHoldEnable.Value = false;

            Tip.Value = false;
        }


        public long Size { get { return 0x100; } }

        public GPIO IRQ { get; private set; }
        public GPIO HOLD { get; private set; }

        private IFlagRegisterField IntEna;
        private IFlagRegisterField AutoHold;
        private IFlagRegisterField Tip;
        private IValueRegisterField Rx;

        // Interrupts
        private IFlagRegisterField SpiIntStatus;
        private IFlagRegisterField SpiIntPending;
        private IFlagRegisterField SpiHoldPending;
        private IFlagRegisterField SpiIntEnable;
        private IFlagRegisterField SpiHoldEnable;

        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        private enum Registers
        {
            ComTx = 0x00,

            ComRx = 0x04,
            ComControl = 0x08,
            ComStatus = 0x0c,
            ComEvStatus = 0x10,
            ComEvPending = 0x14,
            ComEvEnable = 0x18,
        }
    }
}
