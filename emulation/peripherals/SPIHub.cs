//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using Antmicro.Renode.Core;
using System.Linq;
using Antmicro.Renode.Time;
using Antmicro.Renode.Exceptions;
using Antmicro.Migrant.Hooks;
using Antmicro.Renode.Peripherals.SPI;
using System.Collections.Generic;

namespace Antmicro.Renode.Peripherals.UART
{
    public static class SPIHubExtensions
    {
        public static void CreateSPIHub(this Emulation emulation, string name)
        {
            emulation.ExternalsManager.AddExternal(new SPIBus(), name);
        }
    }

    public sealed class SPIBus : IExternal, IHasOwnLife, IConnectable<ISPIPeripheral>
    {
        public SPIBus()
        {
            peripherals = new Dictionary<ISPIPeripheral, Func<byte, byte>>();
            locker = new object();
        }

        public void AttachTo(ISPIPeripheral peripheral)
        {
            lock (locker)
            {
                if (peripherals.ContainsKey(peripheral))
                {
                    throw new RecoverableException("Cannot attach to the provided UART as it is already registered in this hub.");
                }

                var d = (Func<byte, byte>)(x => HandleByteTransmitted(x, peripheral));
                peripherals.Add(peripheral, d);
                // peripheral.CharReceived += d;
            }
        }

        public void Start()
        {
            Resume();
        }

        public void Pause()
        {
            started = false;
        }

        public void Resume()
        {
            started = true;
        }

        public void DetachFrom(ISPIPeripheral peripheral)
        {
            lock (locker)
            {
                if (!peripherals.ContainsKey(peripheral))
                {
                    throw new RecoverableException("Cannot detach the provided SPI peripheral as it is not registered in this bus.");
                }

                // uart.CharReceived -= uarts[uart];
                peripherals.Remove(peripheral);
            }
        }

        private byte HandleByteTransmitted(byte obj, ISPIPeripheral peripheral)
        {
            if (!started)
            {
                return 0;
            }

            lock (locker)
            {
                foreach (var item in uarts.Where(x => x.Key != sender).Select(x => x.Key))
                {
                    item.GetMachine().HandleTimeDomainEvent(item.WriteChar, obj, TimeDomainsManager.Instance.VirtualTimeStamp);
                }
            }
            return 0;
        }

        [PostDeserialization]
        private void ReattachUARTsAfterDeserialization()
        {
            lock (locker)
            {
                foreach (var uart in uarts)
                {
                    uart.Key.CharReceived += uart.Value;
                }
            }
        }

        private bool started;
        private readonly Dictionary<ISPIPeripheral, Func<byte, byte>> peripherals;
        private readonly object locker;
    }
}

