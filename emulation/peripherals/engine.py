#!/usr/bin/env python

from array import array


class OpCode:
    def __init__(self, op):
        self.raw = op

        self.opcode = (op >> 0) & 0x3f

        self.ra = (op >> 6) & 0x1f
        self.ca = ((op >> 11) & 1) == 1

        self.rb = (op >> 12) & 0x1f
        self.cb = ((op >> 17) & 1) == 1

        self.wd = (op >> 18) & 0x1f

        # Convert unsigned value to two's-compliment
        self.immediate = (op >> 23)
        if self.immediate & (1 << 8) != 0:
            self.immediate = self.immediate - (1 << 9)

        self.UINT_MAX = 0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff
        self.UINT_OVERFLOW = 0x10000000000000000000000000000000000000000000000000000000000000000
        self.FIELD_MAX = 0x7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffed

        self.constants = [0, 1,
                          # (A-2)/4
                          121665,
                          # 2^255-19
                          self.FIELD_MAX,
                          # (A+2)/4
                          121666,
                          5, 10, 20, 50, 100, ]
        while len(self.constants) < 32:
            self.constants.append(0)

        self.constant_names = [
            "zero", "one", "am24", "field", "ap24", "five", "ten", "twenty", "fifty", "one hundred"]
        while len(self.constant_names) < 32:
            self.constant_names.append("undef")

    def get_opname(self):
        opcodes = ["PSA", "PSB", "MSK", "XOR", "NOT", "ADD",
                   "SUB", "MUL", "TRD", "BRZ", "FIN", "SHL", "XBT"]
        if self.opcode >= len(opcodes):
            return "UDF"
        return opcodes[self.opcode]

    def ra_name(self):
        if self.ca:
            return "#" + self.constant_names[self.ra].upper()
        return "r{}".format(self.ra)

    def ra_value(self, rf):
        if self.ca:
            return self.constants[self.ra]
        return rf[self.ra]

    def rb_name(self):
        if self.cb:
            return "#" + self.constant_names[self.rb].upper()
        return "r{}".format(self.rb)

    def rb_value(self, rf):
        if self.cb:
            return self.constants[self.rb]
        return rf[self.rb]

    def wd_name(self):
        return "r{}".format(self.wd)

    def start(self, pc, rf):
        # PSA
        if self.opcode == 0:
            rf[self.wd] = self.ra_value(rf)

        # PSB
        elif self.opcode == 1:
            rf[self.wd] = self.rb_value(rf)

        # MSK
        elif self.opcode == 2:
            bit = self.ra_value(rf) & 1
            mask_value = 0
            for x in range(256):
                mask_value |= bit << x
            rf[self.wd] = (mask_value & self.rb_value(rf)) & self.UINT_MAX

        # XOR
        elif self.opcode == 3:
            rf[self.wd] = ((self.UINT_OVERFLOW | self.ra_value(rf))
                           ^ self.rb_value(rf)) & self.UINT_MAX

        # NOT
        elif self.opcode == 4:
            rf[self.wd] = (
                ~(self.UINT_OVERFLOW | self.ra_value(rf))) & self.UINT_MAX

        # ADD
        elif self.opcode == 5:
            rf[self.wd] = (self.ra_value(rf) +
                           self.rb_value(rf)) & self.UINT_MAX

        # SUB
        elif self.opcode == 6:
            rf[self.wd] = (self.ra_value(rf) -
                           self.rb_value(rf)) & self.UINT_MAX

        # MUL
        elif self.opcode == 7:
            rf[self.wd] = (self.ra_value(rf) *
                           self.rb_value(rf)) % self.FIELD_MAX

        # TRD
        elif self.opcode == 8:
            rf[self.wd] = 0
            if self.ra_value(rf) >= self.FIELD_MAX:
                rf[self.wd] = self.FIELD_MAX

        # BRZ
        elif self.opcode == 9:
            if self.ra_value(rf) == 0:
                return pc + self.immediate + 1

        # FIN
        elif self.opcode == 10:
            return None

        # SHL
        elif self.opcode == 11:
            rf[self.wd] = (self.ra_value(rf) << 1) & self.UINT_MAX

        # XBT
        elif self.opcode == 12:
            rf[self.wd] = 0
            if (self.ra_value(rf) & (1 << 254)) != 0:
                rf[self.wd] = 1

        else:
            raise Exception("Unrecognized opcode")

        return pc + 1

    def __repr__(self):
        if self.opcode == 0 or self.opcode == 4 or self.opcode == 8 or self.opcode == 11 or self.opcode == 12:
            return "{} {}, {}".format(self.get_opname().upper(), self.wd_name(), self.ra_name())
        elif self.opcode == 1:
            return "{} {}, {}".format(self.get_opname().upper(), self.wd_name(), self.rb_name())
        elif self.opcode == 9:
            return "{} pc + {}, {}".format(self.get_opname().upper(), self.immediate, self.rb_name())
        elif self.opcode == 10:
            return "{}".format(self.get_opname().upper())
        else:
            return "{} {}, {}, {}".format(self.get_opname().upper(), self.wd_name(), self.ra_name(), self.rb_name())


class EngineJob:
    def __init__(self, ucode, rf):
        self.ucode = ucode
        self.rf = rf

    def spawn(self, uc_start, uc_len):
        pc = uc_start
        while True:
            # If PC is out of bounds, return an error
            if pc < 0 or pc >= len(self.ucode) or pc > uc_start + uc_len:
                return False
            opcode = OpCode(self.ucode[pc])
            # print("    {}".format(opcode))
            pc = opcode.start(pc, self.rf)
            if pc is None:
                return True
            # if pc > uc_start + uc_len:
            #     # New PC is out of bounds
            #     return False

    def print_registers(self, prefix=""):
        for idx, reg in enumerate(self.rf):
            print("{}r{}: {:x}".format(prefix, idx, reg))

    def result(self):
        return self.rf

# ENGINE_WINDOW         0x00
# ENGINE_MPSTART        0x04
# ENGINE_MPLEN          0x08
# ENGINE_CONTROL        0x0c
# ENGINE_MPRESUME       0x10
# ENGINE_POWER          0x14
# ENGINE_STATUS         0x18
# ENGINE_EV_STATUS      0x1c
# ENGINE_EV_PENDING     0x20
# ENGINE_EV_ENABLE      0x24
# ENGINE_INSTRUCTION    0x28
class Engine:
    def __init__(self):
        self.job = None
        self.ev_pending_finished = False
        self.ev_pending_illegal_opcode = False
        self.ev_status_finished = False
        self.ev_status_illegal_opcode = False
        self.ev_enable_finished = False
        self.ev_enabled_illegal_opcode = False
        self.window = 0
        self.mpstart = 0
        self.mplen = 0
        self.control = 0
        self.resume = 0
        self.power = 0
        self.status = 0
        self.instruction = 0

        # There are 16 register windows, each
        # with 32 registers.
        self.rf = []
        for _ in range(16):
            for _ in range(32):
                self.rf.append(0)

    def read(self, addr):
        # ENGINE_CONTROL_GO
        if addr == 0x0c:
            return 0
        return 0
    def write(self, addr, value):
        if addr == 0x00:
            self.window = value & 0xf
        elif addr == 0x0c:
            if value & 1 != 0:
                # GO!
                self.job = EngineJob(self.mpstart, self.rf[self.window:self.window+32])
                self.job.spawn(self.mpstart, self.mplen)
        return

if request.isInit:
    self.NoisyLog("init: %s on ENGINE at 0x%x, value 0x%x" % (str(request.type), request.offset, request.value))
    engine = Engine()
elif request.isRead:
    request.value = engine.read(request.offset)
elif request.isWrite:
    engine.write(request.offset, request.value)
elif request.isUser:
    self.NoisyLog("user: %s on ENGINE at 0x%x, value 0x%x" % (str(request.type), request.offset, request.value))
else:
    self.NoisyLog("Unrecognized request type: %s" % (str(request.type)))
# class TestVectors:
#     def __init__(self, vectors):
#         self.vectors = array('L')
#         """Yield successive 32-bit chunks from lst."""
#         for offset in range(0, len(vectors), 4):
#             v = vectors[offset:offset+4]
#             self.vectors.append(
#                 ((v[0] << 0) & 0x000000ff)
#                 | ((v[1] << 8) & 0x0000ff00)
#                 | ((v[2] << 16) & 0x00ff0000)
#                 | ((v[3] << 24) & 0xff000000)
#             )

#         self.passes = 0
#         self.fails = 0

#     def vector_read(self, word_offset):
#         return self.vectors[word_offset]

#     def vector_read_256(self, word_offset):
#         val = 0
#         for word in range(8):
#             val = val | (
#                 self.vector_read(word_offset) << (32 * word))
#             word_offset += 1
#         return val

#     def insert_at(self, l, offset, value):
#         while len(l) <= offset:
#             l.append(0)
#         l[offset] = value

#     def run(self):
#         test_offset = 0
#         test_suite_number = 0
#         while True:
#             magic_number = self.vector_read(test_offset)
#             if magic_number != 0x56454354:
#                 print(
#                     "Magic number {:08x} doesn't match 0x56454354 -- terminating".format(magic_number))
#                 break
#             print("Test suite #{} at 0x{:x}".format(
#                 test_suite_number, test_offset))
#             test_offset += 1

#             load_addr = (self.vector_read(test_offset) >> 16) & 0xFFFF
#             code_len = self.vector_read(test_offset) & 0xFFFF
#             test_offset += 1
#             num_args = (self.vector_read(test_offset) >> 27) & 0x1F
#             window = (self.vector_read(test_offset) >> 23) & 0xF
#             num_vectors = (self.vector_read(test_offset) >> 0) & 0x3F_FFFF
#             test_offset += 1

#             print("Test will load to address {}. Code is {} words long. There are {} arguments and {} vectors in total.".format(
#                 load_addr, code_len, num_args, num_vectors))

#             job_ucode = array('L')
#             job_rf = []
#             for _ in range(32):
#                 job_rf.append(0)

#             print("Loading vector from  {} .. {}".format(
#                 load_addr, load_addr + code_len))
#             for i in range(load_addr, load_addr + code_len):
#                 self.insert_at(job_ucode, i, self.vector_read(test_offset))
#                 test_offset += 1

#             # Skip over padding
#             test_offset = test_offset + (8 - (test_offset % 8))

#             # copy in the arguments
#             for vector in range(num_vectors):
#                 # a test suite can have numerous vectors against a common code base
#                 for arg_idx in range(num_args):
#                     default_value = self.vector_read_256(test_offset)
#                     test_offset += 8
#                     job_rf[arg_idx] = default_value

#                 job = EngineJob(job_ucode, job_rf)
#                 passed = True
#                 print("Spawning job for suite {}  vector {}".format(
#                     test_suite_number, vector + 1))

#                 if job.spawn(load_addr, code_len):
#                     result = job.result()
#                     expected = self.vector_read_256(test_offset)
#                     test_offset += 8
#                     actual = result[31]

#                     if expected != actual:
#                         print(
#                             "    ERROR: expected: {:x}".format(expected))
#                         print(
#                             "    ERROR: actual:   {:x}".format(actual))
#                         job.print_registers("        ")
#                         passed = False
#                     else:
#                         print("    PASS expected: {:x}".format(expected))
#                 else:
#                     # Skip over the 8 register results
#                     test_offset += 8
#                     print(
#                         "    system error in running test vector: {}/0x{:x}".format(vector, test_offset))
#                     passed = False

#                 if passed:
#                     self.passes += 1
#                 else:
#                     print(
#                         "    arithmetic or system error in running test vector: {}/0x{:x}".format(vector, test_offset))
#                     self.fails += 1
#             test_suite_number += 1


# def run_test_vectors(vector_file):
#     b = open(vector_file, "rb")
#     v = TestVectors(b.read())
#     b.close()
#     v.run()
#     print("{} tests were run".format(v.passes + v.fails))
#     print("    {} PASS".format(v.passes))
#     print("    {} FAIL".format(v.fails))


# run_test_vectors("engine25519_vectors.bin")
