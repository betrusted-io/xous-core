//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.Memory;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.Input
{
    public class BetrustedKbd : IKeyboard, IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public BetrustedKbd(Machine machine)
        {
            this.machine = machine;
            RegistersCollection = new DoubleWordRegisterCollection(this);
            data = new Queue<byte>();
            Reset();
            data.Enqueue((byte)Command.SelfTestPassed);
        }


        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public long Size { get { return 4096; }}
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        public void Press(KeyScanCode scanCode)
        {
            this.Log(LogLevel.Error, "Pressed key {0}", scanCode);
            var key = PS2ScanCodeTranslator.Instance.GetCode(scanCode);
            data.Enqueue((byte)(key & 0x7f));
            NotifyParent();
        }

        public void Release(KeyScanCode scanCode)
        {
            this.Log(LogLevel.Noisy, "Released key {0}", scanCode);
            var key = PS2ScanCodeTranslator.Instance.GetCode(scanCode);
            data.Enqueue((byte)Command.Release);
            data.Enqueue((byte)(key & 0x7f));
            NotifyParent();
        }

        public void Reset()
        {
            data.Clear();
        }

        public IPS2Controller Controller { get; set; }

        private void SendAck()
        {
            data.Enqueue((byte)Command.Acknowledge);
            NotifyParent();
        }

        private void NotifyParent()
        {
            if(Controller != null)
            {
                if(data.Count > 0)
                {
                    Controller.Notify();
                }
            }
            else
            {
                this.Log(LogLevel.Noisy, "PS2 device not connected to any controller issued an update.");
            }
        }

        private readonly Queue<byte> data;
        private readonly Machine machine;
        private const ushort deviceId = 0xABBA;

        private enum Command
        {
            Reset = 0xFF,
            Acknowledge = 0xFA,
            ReadId = 0xF2,
            SetResetLeds = 0xED,
            Release = 0xF0,
            SelfTestPassed = 0xAA,
        }
    }
}
