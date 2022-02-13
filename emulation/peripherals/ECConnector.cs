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

namespace Antmicro.Renode.Peripherals.EC
{
    public interface IECPeripheral : IPeripheral, IEmulationElement, IInterestingType, IAnalyzable
    {
        void FinishTransmission();
        ushort Transmit(ushort data);
    }

    public static class ECConnectorExtensions
    {
        public static void CreateECConnector(this Emulation emulation, string name)
        {
            emulation.ExternalsManager.AddExternal(new ECConnector(), name);
        }
    }

    public class ECConnector : IExternal, IECPeripheral, IConnectable<IECPeripheral>, IConnectable<NullRegistrationPointPeripheralContainer<IECPeripheral>>
    {
        public void Reset()
        {
            peripheralOutput.Clear();
        }

        public void AttachTo(NullRegistrationPointPeripheralContainer<IECPeripheral> controller)
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

        public void AttachTo(IECPeripheral peripheral)
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

        public void DetachFrom(NullRegistrationPointPeripheralContainer<IECPeripheral> controller)
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

        public void DetachFrom(IECPeripheral peripheral)
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

        public ushort Transmit(ushort data)
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

        private void InnerTransmit(ushort data)
        {
            peripheralOutput.Enqueue(peripheral.Transmit(data));
        }

        private NullRegistrationPointPeripheralContainer<IECPeripheral> controller;
        private IECPeripheral peripheral;
        private readonly Queue<ushort> peripheralOutput = new Queue<ushort>();
        private readonly object locker = new object();
    }
}
