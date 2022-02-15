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

namespace Antmicro.Renode.Peripherals.SPI
{
    public static class SPIConnectorExtensions
    {
        public static void CreateSPIConnector(this Emulation emulation, string name)
        {
            emulation.ExternalsManager.AddExternal(new SPIConnector(), name);
        }
    }

    public class SPIConnector : IExternal, ISPIPeripheral, IConnectable<ISPIPeripheral>, IConnectable<NullRegistrationPointPeripheralContainer<ISPIPeripheral>>
    {
        public void Reset()
        {
            peripheralOutput.Clear();
        }

        public void AttachTo(NullRegistrationPointPeripheralContainer<ISPIPeripheral> controller)
        {
            lock(locker)
            {
                if(controller == this.controller)
                {
                    throw new RecoverableException("Cannot attach to the provided peripheral as it is already registered in this connector.");
                }
                else if(this.controller != null)
                {
                    this.Log(LogLevel.Warning, "Overwriting controller connection.");
                }

                this.controller?.Unregister(this);
                this.controller = controller;
                this.controller.Register(this, NullRegistrationPoint.Instance);
            }
        }

        public void AttachTo(ISPIPeripheral peripheral)
        {
            lock(locker)
            {
                if(peripheral == this.peripheral)
                {
                    throw new RecoverableException("Cannot attach to the provided peripheral as it is already registered in this connector.");
                }
                else if(this.peripheral != null)
                {
                    this.Log(LogLevel.Warning, "Overwriting peripheral connection.");
                }
                this.peripheral = peripheral;
            }
        }

        public void DetachFrom(NullRegistrationPointPeripheralContainer<ISPIPeripheral> controller)
        {
            lock(locker)
            {
                if(controller == this.controller)
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

        public void DetachFrom(ISPIPeripheral peripheral)
        {
            lock(locker)
            {
                if(peripheral == this.peripheral)
                {
                    peripheral = null;
                    peripheralOutput.Clear();
                }
                else
                {
                    throw new RecoverableException("Cannot detach from the provided peripheral as it is not registered in this connector.");
                }
            }
        }

        public byte Transmit(byte data)
        {
            lock(locker)
            {
                if(peripheral == null)
                {
                    this.Log(LogLevel.Warning, "Controller sent data (0x{0:X}), but peripheral is not connected.", data);
                    return 0x0;
                }
                peripheral.GetMachine().HandleTimeDomainEvent(InnerTransmit, data, TimeDomainsManager.Instance.VirtualTimeStamp);
                return peripheralOutput.Count > 0 ? peripheralOutput.Dequeue() : (byte)0x0;
            }
        }

        public void FinishTransmission()
        {
            lock(locker)
            {
                peripheral.GetMachine().HandleTimeDomainEvent<object>(_ => peripheral.FinishTransmission(), null, TimeDomainsManager.Instance.VirtualTimeStamp);
            }
        }

        private void InnerTransmit(byte data)
        {
            peripheralOutput.Enqueue(peripheral.Transmit(data));
        }

        private NullRegistrationPointPeripheralContainer<ISPIPeripheral> controller;
        private ISPIPeripheral peripheral;
        private readonly Queue<byte> peripheralOutput = new Queue<byte>();
        private readonly object locker = new object();
    }
}
