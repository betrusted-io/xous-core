//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Utilities.Binding;

namespace Antmicro.Renode.Peripherals.Miscellaneous.Betrusted
{

    public class BetrustedWfi : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        [Import]
        private Action TlibEnterWfi;

        public BetrustedWfi(Machine machine)
        {
            this.RegistersCollection = new DoubleWordRegisterCollection(this);
            DefineRegisters();
            Reset();
        }

        private void DefineRegisters()
        {
            Registers.WFI.Define32(this)
                .WithFlag(0, writeCallback: (_, val) => { if (val) { /*TlibEnterWfi(); */ } })
            ;

            Registers.IGNORE_LOCKED.Define32(this)
                .WithFlag(0, name: "IGNORE_LOCKED", changeCallback: (_, __) => { })
            ;
        }

        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public long Size { get { return 4096; } }
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        public void Reset()
        {
        }

        private enum Registers
        {
            WFI = 0x00,
            IGNORE_LOCKED = 0x04
        }
    }
}
