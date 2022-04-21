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

namespace Antmicro.Renode.Peripherals.Input
{
    class KeyMap
    {
        public UInt32[] rows;
        public KeyMap(UInt32 row0, UInt32 row1, UInt32 row2, UInt32 row3, UInt32 row4, UInt32 row5, UInt32 row6, UInt32 row7, UInt32 row8)
        {
            this.rows = new uint[9];
            this.rows[0] = row0;
            this.rows[1] = row1;
            this.rows[2] = row2;
            this.rows[3] = row3;
            this.rows[4] = row4;
            this.rows[5] = row5;
            this.rows[6] = row6;
            this.rows[7] = row7;
            this.rows[8] = row8;
        }
    }

    public class BetrustedKbd : IKeyboard, IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public BetrustedKbd(Machine machine)
        {
            this.rows = new uint[9];
            this.machine = machine;
            RegistersCollection = new DoubleWordRegisterCollection(this);
            this.IRQ = new GPIO();
            injectedKeys = new Queue<char>();
            DefineRegisters();
            Reset();
        }

        private void DefineRegisters()
        {
            Registers.UART_CHAR.Define(this)
                .WithValueField(0, 9,
                    valueProviderCallback: _ =>
                    {
                        var ret = injectedKey;
                        // If there is a key, dequeue it.
                        if (injectedKeys.Count > 0)
                        {
                            injectedKey = ((uint)injectedKeys.Dequeue());
                            ret = injectedKey | 0x100;
                            UpdateInterrupts();
                        }
                        return ret;
                    },
                    writeCallback: (_, value) =>
                    {
                        if (value != 0)
                        {
                            injectedKeys.Enqueue((char)value);
                            UpdateInterrupts();
                        }
                    }
                )
            ;
            Registers.ROW0DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[0]; })
            ;
            Registers.ROW1DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[1]; })
            ;
            Registers.ROW2DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[2]; })
            ;
            Registers.ROW3DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[3]; })
            ;
            Registers.ROW4DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[4]; })
            ;
            Registers.ROW5DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[5]; })
            ;
            Registers.ROW6DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[6]; })
            ;
            Registers.ROW7DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[7]; })
            ;
            Registers.ROW8DAT.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return this.rows[8]; })
            ;
            Registers.EV_STATUS.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "KEYPRESSED", valueProviderCallback: _ => irqKeyPressedStatus)
                .WithFlag(1, FieldMode.Read, name: "INJECT", valueProviderCallback: _ => injectedKeys.Count > 0)
            ;

            Registers.EV_PENDING.Define32(this)
                .WithFlag(0, out irqKeyPressedPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "KEYPRESSED", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out irqInjectPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "INJECT", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EV_ENABLE.Define32(this)
                .WithFlag(0, out irqKeyPressedEnabled, name: "KEYPRESSED", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out irqInjectEnabled, name: "INJECT", changeCallback: (_, __) => UpdateInterrupts())
            ;
            Registers.ROWCHANGE.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => this.rowChange)
            ;
        }

        private void UpdateInterrupts()
        {
            if (this.irqKeyPressedStatus && this.irqKeyPressedEnabled.Value)
            {
                this.irqKeyPressedPending.Value = true;
            }
            if (injectedKeys.Count > 0 && this.irqInjectEnabled.Value)
            {
                this.irqInjectPending.Value = true;
            }
            IRQ.Set((this.irqKeyPressedPending.Value && this.irqKeyPressedEnabled.Value)
                 || (this.irqInjectPending.Value && this.irqInjectEnabled.Value));
        }

        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public void InjectKey(char key)
        {
            injectedKeys.Enqueue(key);
            UpdateInterrupts();
        }

        public void InjectString(String s)
        {
            foreach (var letter in s.ToCharArray())
            {
                injectedKeys.Enqueue(letter);
            }
            UpdateInterrupts();
        }

        public void InjectLine(String s)
        {
            foreach (var letter in s.ToCharArray())
            {
                injectedKeys.Enqueue(letter);
            }
            injectedKeys.Enqueue('\r');
            UpdateInterrupts();
        }

        public long Size { get { return 4096; } }
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        private void UpdateKeyList(KeyScanCode scanCode, bool isPress)
        {
            // Ignore duplicate events which happen due to key repeat.
            if ((isPress && this.PressedKeys.ContainsKey(scanCode))
            || (!isPress && !this.PressedKeys.ContainsKey(scanCode)))
            {
                return;
            }

            // If we can't handle this key, don't process it.
            if (!MatrixMapping.ContainsKey(scanCode))
            {
                this.Log(LogLevel.Debug, "No key mapping found for {0}", scanCode);
                return;
            }

            // Add or remove this key from the list of currently-active keys
            this.Log(LogLevel.Debug, "Got new key -- press? {0} : {1}", isPress, scanCode);
            if (isPress)
            {
                this.PressedKeys.Add(scanCode, true);
            }
            else
            {
                this.PressedKeys.Remove(scanCode);
            }

            // Reconstruct the keyboard matrix press state
            for (var i = 0; i < this.rows.Length; i++)
            {
                this.rows[i] = 0;
            }

            // Simulate the keyboard matrix by ORing all pressed keys.
            foreach (var key in this.PressedKeys)
            {
                KeyMap result;
                MatrixMapping.TryGetValue(key.Key, out result);
                for (var i = 0; i < this.rows.Length; i++)
                {
                    this.rows[i] |= result.rows[i];
                }
            }
            // for (var i = 0; i <= 8; i++)
            // {
            //     this.Log(LogLevel.Debug, "    rows[{0}] = {1}", i, rows[i]);
            // }

            // Update the `rowChange` field if no interrupt is pending.
            if (!this.irqInjectPending.Value)
            {
                for (var i = 0; i <= 8; i++)
                {
                    if (this.rows[i] != 0)
                    {
                        this.rowChange |= (1u << i);
                    }
                }
            }
            this.irqKeyPressedStatus = true;
            this.UpdateInterrupts();
            this.irqKeyPressedStatus = false;

            // Wake the machine if it's suspended.
            if (PressedKeys.ContainsKey(KeyScanCode.F1)
             && PressedKeys.ContainsKey(KeyScanCode.F4)
             && (scanCode == KeyScanCode.F1 || scanCode == KeyScanCode.F4)
             && isPress
             && machine.IsPaused
            )
            {
                this.Log(LogLevel.Debug, "Resuming machine");
                machine.Start();
            }
            return;
        }

        public void Press(KeyScanCode scanCode)
        {
            this.UpdateKeyList(scanCode, true);
        }

        public void Release(KeyScanCode scanCode)
        {
            this.UpdateKeyList(scanCode, false);
        }

        public void Reset()
        {
            for (var i = 0; i < this.rows.Length; i++)
            {
                this.rows[i] = 0;
            }
            irqKeyPressedStatus = false;
            injectedKeys.Clear();
            PressedKeys.Clear();
            irqKeyPressedEnabled.Value = false;
            irqKeyPressedPending.Value = false;
            irqInjectEnabled.Value = false;
            irqInjectPending.Value = false;
            RegistersCollection.Reset();
        }

        private Dictionary<KeyScanCode, bool> PressedKeys = new Dictionary<KeyScanCode, bool> { };

        private readonly Dictionary<KeyScanCode, KeyMap> MatrixMapping = new Dictionary<KeyScanCode, KeyMap> {
            {KeyScanCode.Number1, new KeyMap(1<<0, 0, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.Number2, new KeyMap(1<<1, 0, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.Number3, new KeyMap(1<<2, 0, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.Number4, new KeyMap(1<<3, 0, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.Number5, new KeyMap(1<<4, 0, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.Number6, new KeyMap(0, 0, 0, 0, 1<<5, 0, 0, 0, 0)},
            {KeyScanCode.Number7, new KeyMap(0, 0, 0, 0, 1<<6, 0, 0, 0, 0)},
            {KeyScanCode.Number8, new KeyMap(0, 0, 0, 0, 1<<7, 0, 0, 0, 0)},
            {KeyScanCode.Number9, new KeyMap(0, 0, 0, 0, 1<<8, 0, 0, 0, 0)},
            {KeyScanCode.Number0, new KeyMap(0, 0, 0, 0, 1<<9, 0, 0, 0, 0)},

            {KeyScanCode.Q, new KeyMap(0, 1<<0, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.W, new KeyMap(0, 1<<1, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.E, new KeyMap(0, 1<<2, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.R, new KeyMap(0, 1<<3, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.T, new KeyMap(0, 1<<4, 0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.Y, new KeyMap(0, 0, 0, 0, 0, 1<<5, 0, 0, 0)},
            {KeyScanCode.U, new KeyMap(0, 0, 0, 0, 0, 1<<6, 0, 0, 0)},
            {KeyScanCode.I, new KeyMap(0, 0, 0, 0, 0, 1<<7, 0, 0, 0)},
            {KeyScanCode.O, new KeyMap(0, 0, 0, 0, 0, 1<<8, 0, 0, 0)},
            {KeyScanCode.P, new KeyMap(0, 0, 0, 0, 0, 1<<9, 0, 0, 0)},

            {KeyScanCode.A, new KeyMap(0, 0, 1<<0, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.S, new KeyMap(0, 0, 1<<1, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.D, new KeyMap(0, 0, 1<<2, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.F, new KeyMap(0, 0, 1<<3, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.G, new KeyMap(0, 0, 1<<4, 0, 0, 0, 0, 0, 0)},
            {KeyScanCode.H, new KeyMap(0, 0, 0, 0, 0, 0, 1<<5, 0, 0)},
            {KeyScanCode.J, new KeyMap(0, 0, 0, 0, 0, 0, 1<<6, 0, 0)},
            {KeyScanCode.K, new KeyMap(0, 0, 0, 0, 0, 0, 1<<7, 0, 0)},
            {KeyScanCode.L, new KeyMap(0, 0, 0, 0, 0, 0, 1<<8, 0, 0)},
            {KeyScanCode.BackSpace, new KeyMap(0, 0, 0, 0, 0, 0, 1<<9, 0, 0)},

            {KeyScanCode.Tilde, new KeyMap(0, 0, 0, 1<<0, 0, 0, 0, 0, 0)},
            {KeyScanCode.Z, new KeyMap(0, 0, 0, 1<<1, 0, 0, 0, 0, 0)},
            {KeyScanCode.X, new KeyMap(0, 0, 0, 1<<2, 0, 0, 0, 0, 0)},
            {KeyScanCode.C, new KeyMap(0, 0, 0, 1<<3, 0, 0, 0, 0, 0)},
            {KeyScanCode.V, new KeyMap(0, 0, 0, 1<<4, 0, 0, 0, 0, 0)},
            {KeyScanCode.B, new KeyMap(0, 0, 0, 0, 0, 0, 0, 1<<5,  0)},
            {KeyScanCode.N, new KeyMap(0, 0, 0, 0, 0, 0, 0, 1<<6,  0)},
            {KeyScanCode.M, new KeyMap(0, 0, 0, 0, 0, 0, 0, 1<<7,  0)},
            {KeyScanCode.OemQuestion, new KeyMap(0, 0, 0, 0, 0, 0, 0, 1<<8, 0)},
            {KeyScanCode.Enter, new KeyMap(0, 0, 0, 0, 0, 0, 0, 1<<9,  0)},

            {KeyScanCode.ShiftL, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<5)},
            {KeyScanCode.OemComma, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<6)},
            {KeyScanCode.Space, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<7)},
            {KeyScanCode.OemPeriod, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<8)},
            {KeyScanCode.ShiftR, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<9)},

            {KeyScanCode.F1, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<0)},
            {KeyScanCode.F2, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<1)},
            {KeyScanCode.F3, new KeyMap(0, 0, 0, 1<<8, 0, 0, 0, 0, 0)},
            {KeyScanCode.F4, new KeyMap(0, 0, 0, 1<<9, 0, 0, 0, 0, 0)},
            {KeyScanCode.Up, new KeyMap(0, 0, 0, 0, 0, 0, 1<<4, 0, 0)},
            {KeyScanCode.Down, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<2)},
            {KeyScanCode.Left, new KeyMap(0, 0, 0, 0, 0, 0, 0, 0, 1<<3)},
            {KeyScanCode.Right, new KeyMap(0, 0, 0, 1<<6, 0, 0, 0, 0, 0)},

            {KeyScanCode.KeypadEnter, new KeyMap(0, 0, 0, 0, 0, 1<<2, 0, 0, 0)},
            {KeyScanCode.OemPipe, new KeyMap(0, 0, 0, 0, 0, 1<<2, 0, 0, 0)},
            {KeyScanCode.Home, new KeyMap(0, 0, 0, 0, 0, 1<<2, 0, 0, 0)},
        };

        private readonly Machine machine;
        private UInt32[] rows;

        public GPIO IRQ { get; private set; }
        private IFlagRegisterField irqKeyPressedEnabled;
        private IFlagRegisterField irqKeyPressedPending;
        private bool irqKeyPressedStatus;
        private IFlagRegisterField irqInjectEnabled;
        private IFlagRegisterField irqInjectPending;
        private UInt32 rowChange;
        private UInt32 injectedKey;
        private readonly Queue<char> injectedKeys;
        private enum Registers
        {
            UART_CHAR = 0x0,
            ROW0DAT = 0x04,
            ROW1DAT = 0x08,
            ROW2DAT = 0x0C,
            ROW3DAT = 0x10,
            ROW4DAT = 0x14,
            ROW5DAT = 0x18,
            ROW6DAT = 0x1C,
            ROW7DAT = 0x20,
            ROW8DAT = 0x24,
            EV_STATUS = 0x28,
            EV_PENDING = 0x2C,
            EV_ENABLE = 0x30,
            ROWCHANGE = 0x34,
        }
    }
}
