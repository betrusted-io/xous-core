//
// Copyright (c) 2010 - 2019 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//

using System;
using System.Linq;
using Antmicro.Renode.Backends.Display;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.Memory;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.Video
{
    public class BetrustedLCD : AutoRepaintingVideo, IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public BetrustedLCD(Machine machine) : base(machine)
        {
            this.machine = machine;

            RegistersCollection = new DoubleWordRegisterCollection(this);
            Reconfigure(336, 536, PixelFormat.RGB565, true);
            for (int i = 0; i < buffer.Length; i++) buffer[i] = 0;
            DoRepaint();
            DefineRegisters();
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

        public override void Reset()
        {
            RegistersCollection.Reset();
        }

        public long Size { get{ return  0x800; }}
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        protected override void Repaint()
        {
            var newbuf = new Byte[44*Height];
            machine.SystemBus.ReadBytes(bufferAddress, newbuf.Length, newbuf, 0);
            for (int y = 0; y < Height; y++) {
                if (!updateDirty || updateAll || ((newbuf[y*44+0x2a] & 0x1) == 0x1)) for (int x = 0; x < Width; x++) {
                    if (((newbuf[((x+y*44*8))/8] >> (x%8))&1) > 0) {
                        buffer[2*(x+y*Width)] = 0xFF;
                        buffer[2*(x+y*Width)+1] = 0xFF;
                    } else {
                        buffer[2*(x+y*Width)] = 0x0;
                        buffer[2*(x+y*Width)+1] = 0x0;
                    }
                }
            }
        }

        private void DefineRegisters()
        {
            Registers.COMMAND.Define(this)
                .WithValueField(0, 32, writeCallback: (_, val) =>
                        {
                        updateDirty = (val & 0x1) == 0x1;
                        updateAll = (val & 0x2) == 0x2;
                        DoRepaint();
                        })
            ;
            Registers.BUSY.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return 0; })
            ;
        }

        private bool updateDirty = false;
        private bool updateAll = false;

        private uint bufferAddress = 0xB0000000;

        private readonly Machine machine;

        private enum Registers
        {
            COMMAND = 0x0,
            BUSY = 0x04
        }
    }
}
