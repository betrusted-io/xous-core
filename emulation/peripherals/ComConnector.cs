//
// Copyright (c) 2010-2021 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Exceptions;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Time;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.SPI.Betrusted
{
    public interface IComPeripheral : IPeripheral
    {
        ushort Transmit(ushort data);
        bool HasData();
        void FinishTransmission();
    }
    public interface IComController : IPeripheral, IGPIOReceiver
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
                    if (gpio.Item1 == "EC_INTERRUPT")
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
                    // this.Log(LogLevel.Warning, "Setting hold value to: {0}", value);
                    controller.GetMachine().HandleTimeDomainEvent(holdGpio.Set, value, false);
                }
            }
            else if (number == 1)
            {
                // "Interrupt" pin
                if (interruptGpio != null)
                {
                    // controller.GetMachine().HandleTimeDomainEvent(holdGpio.Set, value, new TimeStamp(default(TimeInterval), EmulationManager.ExternalWorld));
                    // this.Log(LogLevel.Warning, "Setting interurpt value to: {0}", value);
                    controller.GetMachine().HandleTimeDomainEvent(interruptGpio.Set, value, false);
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
                ushort result = peripheral.Transmit(data);

                // Update the hold GPIO in this same domain. It may get updated slightly in the future
                // by the `OnGPIO` call above, but update it in this cycle to avoid latency issues.
                if (holdGpio != null)
                {
                    holdGpio.Set(!peripheral.HasData());
                }
                return result;
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
}
