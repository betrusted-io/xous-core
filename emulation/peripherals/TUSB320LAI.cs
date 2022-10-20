//
// Copyright (c) 2010-2021 Antmicro
//
//  This file is licensed under the MIT License.
//  Full license text is available in 'licenses/MIT.txt'.
//

using Antmicro.Renode.Logging;
using Antmicro.Renode.Utilities;
using Antmicro.Renode.Peripherals.I2C;

namespace Antmicro.Renode.Peripherals.Mocks.Betrusted
{
    public class TUSB320LAI : II2CPeripheral
    {
        public TUSB320LAI()
        {
            Reset();
        }

        public void Write(byte[] data)
        {
            this.Log(LogLevel.Debug, "Written {0} bytes of data: {1}", data.Length, Misc.PrettyPrintCollectionHex(data));
            if (data.Length == 2)
            {
                this.lastAddress = data[1];
            }
            buffer = data;
        }

        public byte[] Read(int count = 1)
        {
            this.Log(LogLevel.Debug, "Reading {0} bytes", count);
            var result = new byte[count];

            if (lastAddress == 0)
            {
                result[0] = 0x30;
            }
            else if (lastAddress == 1)
            {
                result[0] = 0x32;
            }
            else if (lastAddress == 2)
            {
                result[0] = 0x33;
            }
            else if (lastAddress == 3)
            {
                result[0] = 0x42;
            }
            else if (lastAddress == 4)
            {
                result[0] = 0x53;
            }
            else if (lastAddress == 5)
            {
                result[0] = 0x55;
            }
            else if (lastAddress == 6)
            {
                result[0] = 0x54;
            }
            else if (lastAddress == 7)
            {
                result[0] = 0x00;
            }
            else
            {
                this.Log(LogLevel.Error, "Reading register {0} ({1} bytes)", lastAddress, count);
                for (var i = 0; i < result.Length; i++)
                {
                    result[i] = (i < buffer.Length)
                        ? buffer[i]
                        : (byte)0;
                }
            }
            lastAddress += 1;

            return result;
        }

        public void FinishTransmission()
        {
            this.Log(LogLevel.Debug, "Finishing transmission");
        }

        public void Reset()
        {
            buffer = new byte[0];
        }

        private byte[] buffer;
        private byte lastAddress = 0;
    }
}
