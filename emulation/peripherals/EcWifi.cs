//
// Copyright (c) 2010-2021 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Core.Structure.Registers;

// This SPI block is optimized for the WF200 chip. It always transfers 16 bits 
namespace Antmicro.Renode.Peripherals.SPI.Betrusted
{
    public class EcWifi : NullRegistrationPointPeripheralContainer<ISPIPeripheral>, IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public EcWifi(Machine machine) : base(machine)
        {
            IRQ = new GPIO();
            RegistersCollection = new DoubleWordRegisterCollection(this);

            Registers.Tx.Define32(this)
                .WithValueField(0, 16, out Tx, FieldMode.Write, writeCallback: (_, __) => TxFull.Value = true, name: "tx")
            ;

            Registers.Rx.Define32(this)
                .WithValueField(0, 16, out Rx, FieldMode.Read, name: "rx")
            ;

            Registers.Cs.Define32(this)
                .WithFlag(0, FieldMode.Write, writeCallback: (_, new_cs) =>
                {
                    // If we transition low -> high, finish the transaction -- note that
                    // CS logic is inverted in this block (i.e. writing a 1 drives CS low)
                    if (!new_cs && cs)
                    {
                        if (RegisteredPeripheral != null)
                        {
                            RegisteredPeripheral.FinishTransmission();
                        }
                    }
                    cs = new_cs;
                }, name: "cs")
            ;

            Registers.Control.Define32(this)
                .WithFlag(0, FieldMode.Read | FieldMode.Write, writeCallback: (_, go) =>
                {
                    if (go)
                    {
                        if (RegisteredPeripheral == null)
                        {
                            this.Log(LogLevel.Error, "tried to transfer data with no wifi chip attached");
                            return;
                        }
                        Rx.Value = ((uint)RegisteredPeripheral.Transmit((byte)(Tx.Value >> 0))) << 0;
                        Rx.Value |= ((uint)RegisteredPeripheral.Transmit((byte)(Tx.Value >> 8))) << 8;
                        TxFull.Value = false;
                        machine.Log(LogLevel.Noisy, "SoC sent {0:X04} received {1:X04}",
                                    Tx.Value, Rx.Value);
                    }
                }, name: "go")
            ;
            Registers.Status.Define32(this)
                .WithFlag(0, out Tip, FieldMode.Read, name: "tip")
                .WithFlag(1, out TxFull, FieldMode.Read, name: "txfull")
            ;

            Registers.Wifi.Define32(this)
                .WithFlag(0, FieldMode.Read | FieldMode.Write, writeCallback: (_, val) =>
                {
                    if (val)
                    {
                        Reset();
                    }
                }, name: "reset")
                .WithFlag(1, out PaEnable, FieldMode.Read | FieldMode.Write, name: "pa_ena")
                .WithFlag(2, FieldMode.Read | FieldMode.Write, name: "wakeup")
            ;

            Registers.EvStatus.Define32(this)
                .WithFlag(0, out WirqStatus, FieldMode.Read, name: "wirq")
            ;

            Registers.EvPending.Define32(this)
                .WithFlag(0, out WirqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "wirq", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EvEnable.Define32(this)
                .WithFlag(0, out WirqEnable, name: "wirq", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Reset();
        }

        private void UpdateInterrupts()
        {
            if (IRQ.IsSet)
            {
                WirqPending.Value = true;
            }
            IRQ.Set((WirqPending.Value && WirqEnable.Value));
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
            WirqStatus.Value = false;
            WirqPending.Value = false;
            WirqEnable.Value = false;

            cs = false;
            PaEnable.Value = false;
        }


        public long Size { get { return 0x100; } }

        public GPIO IRQ { get; private set; }

        private IFlagRegisterField PaEnable;
        private bool cs;
        private IValueRegisterField Tx;
        private IValueRegisterField Rx;
        private IFlagRegisterField TxFull;
        private IFlagRegisterField Tip;

        // Interrupts
        private IFlagRegisterField WirqStatus;
        private IFlagRegisterField WirqPending;
        private IFlagRegisterField WirqEnable;

        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        private enum Registers
        {
            Tx = 0x00,

            Rx = 0x04,
            Cs = 0x08,
            Control = 0x0c,
            Status = 0x10,
            Wifi = 0x14,
            EvStatus = 0x18,
            EvPending = 0x1c,
            EvEnable = 0x20,
        }
    }
}
