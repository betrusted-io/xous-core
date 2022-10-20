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
    public class BQ24157 : II2CPeripheral
    {
        public BQ24157()
        {
            Reset();
        }

        public void Write(byte[] data)
        {
            this.Log(LogLevel.Debug, "Written {0} bytes of data: {1}", data.Length, Misc.PrettyPrintCollectionHex(data));
            buffer = data;
        }

        public byte[] Read(int count = 1)
        {
            this.Log(LogLevel.Debug, "Reading {0} bytes", count);
            var result = new byte[count];
            for(var i = 0; i < result.Length; i++)
            {
                result[i] = (i < buffer.Length)
                    ? buffer[i]
                    : (byte)0;
            }

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
    }
}
