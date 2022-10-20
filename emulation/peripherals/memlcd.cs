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
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Video.Betrusted
{

    public class VideoRam : IBytePeripheral, IDoubleWordPeripheral, IWordPeripheral, IKnownSize
    {
        public VideoRam(Machine machine, BetrustedLCD lcd, long size)
        {
            this.machine = machine;
            this.lcd = lcd;
            this.size = size;
            this.data = new byte[size];
        }

        private Machine machine;
        private BetrustedLCD lcd;
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

    public class BetrustedLCD : AutoRepaintingVideo, IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public BetrustedLCD(Machine machine, uint memAddr, uint memSize) : base(machine)
        {
            this.machine = machine;
            this.videoRam = new VideoRam(machine, this, memSize);
            this.bufferAddress = memAddr;
            machine.SystemBus.Register(this.videoRam, new BusRangeRegistration(memAddr, memSize));

            RegistersCollection = new DoubleWordRegisterCollection(this);
            Reconfigure(336, 536, PixelFormat.RGB565, true);
            for (int i = 0; i < buffer.Length; i++)
            {
                buffer[i] = 0;
            }
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
            this.videoRam.Reset();
        }

        public long Size { get { return 0x1000; } }
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        protected override void Repaint()
        {
            for (int y = 0; y < Height; y++)
            {
                // We should redraw the line if:
                // 1) The `updateDirty` bit is set and the current line is dirty, or
                // 2) The `updateAll` bit is set.
                bool shouldRedrawLine = updateAll;
                foreach (int i in Enumerable.Range(41, 2))
                {
                    if (shouldRedrawLine)
                    {
                        break;
                    }
                    // A line is considered "dirty" if any bit following the pixel data is
                    // nonzero. Because each line is 336 pixels wide (which yields 42 bytes
                    // of data per line), the remaining 2 bytes indicate whether the line
                    // is dirty or not.
                    if (updateDirty && (this.videoRam.data[y * 44 + i] != 0))
                    {
                        shouldRedrawLine = true;
                    }
                }
                if (shouldRedrawLine)
                {
                    for (int x = 0; x < Width; x++)
                    {
                        if ((devBoot) && (y == devBootLine) && ((x >> 1) & 2) != 0)
                        {
                            buffer[2 * (x + y * Width)] = 0x00;
                            buffer[2 * (x + y * Width) + 1] = 0x00;
                        }
                        else
                        {
                            if (((this.videoRam.data[(x + y * 44 * 8) / 8] >> (x % 8)) & 1) > 0)
                            {
                                buffer[2 * (x + y * Width)] = 0xFF;
                                buffer[2 * (x + y * Width) + 1] = 0xFF;
                            }
                            else
                            {
                                buffer[2 * (x + y * Width)] = 0x0;
                                buffer[2 * (x + y * Width) + 1] = 0x0;
                            }
                        }
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
            Registers.PRESCALER.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return 0; })
            ;
            Registers.DEVBOOT.Define(this)
                .WithFlag(0, name: "DEVBOOT", valueProviderCallback: _ => { return devBoot; }, changeCallback: (_, val) => { if (val) { devBoot = true; } })
            ;
        }

        private const int devBootLine = 7;
        private bool devBoot = false;
        private bool updateDirty = false;
        private bool updateAll = false;
        private VideoRam videoRam;

        private uint bufferAddress;

        private readonly Machine machine;

        private enum Registers
        {
            COMMAND = 0x0,
            BUSY = 0x04,
            PRESCALER = 0x08,
            DEVBOOT = 0x18,
        }
    }
}
