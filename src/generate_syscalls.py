#!/usr/bin/env python3

import assembly_templates
from io import StringIO
import os
import string
import sys
import syscalls

def arch_syscall_number(arch, syscall):
    s = getattr(syscall[1], arch)
    if s == None:
        s = -1
    return s

def write_syscall_consts(f, arch):
    undefined_syscall = -1
    for name, obj in sorted(syscalls.all(), key=lambda x: arch_syscall_number(arch, x)):
        syscall_number = getattr(obj, arch)
        if syscall_number is not None:
            enum_number = syscall_number
        else:
            enum_number = undefined_syscall
            undefined_syscall -= 1
        f.write("pub const %s : i32 = %d;\n" % (name.upper(), enum_number))
    # @TODO.
    # f.write("pub const SYSCALL_COUNT,\n")
    f.write("\n")

def write_syscall_consts_for_tests(f, arch):
    undefined_syscall = -1
    for name, obj in sorted(syscalls.all(), key=lambda x: arch_syscall_number(arch, x)):
        syscall_number = getattr(obj, arch)
        if syscall_number is not None:
            enum_number = syscall_number
        else:
            enum_number = undefined_syscall
            undefined_syscall -= 1
        f.write("pub const RR_%s = %d,\n" % (name.upper(), enum_number))
    f.write("\n")

def write_syscallname_arch(f, arch):
    f.write("use std::fmt::Write;\n")
    if arch == 'x86':
        specializer = 'x86_arch'
    elif arch == 'x64':
        specializer = 'x64_arch'
    f.write("use crate:: %s;\n" % (specializer))
    f.write("\n");

    f.write("pub fn syscallname_arch(syscall : i32) -> String {\n")
    f.write("  match syscall {\n");
    def write_case(name):
        f.write("    %(specializer)s::%(syscall_upper)s => \"%(syscall)s\".into(),\n"
                % { 'specializer': specializer, 'syscall_upper': name.upper(), 'syscall': name })
    for name, _ in syscalls.for_arch(arch):
        write_case(name)
    f.write("    _ => {\n")
    f.write("      let mut s = String::new();\n")
    f.write("      write!(s, \"<unknown-syscall-{}>\", syscall).unwrap();\n")
    f.write("      s\n")
    f.write("    }\n")
    f.write("  }\n")
    f.write("}\n")
    f.write("\n")

def write_syscall_record_cases(f):
    def write_recorder_for_arg(syscall, arg):
        arg_descriptor = getattr(syscall, 'arg' + str(arg), None)
        if isinstance(arg_descriptor, str):
            f.write("    syscall_state.reg_parameter<%s>(%d);\n"
                    % (arg_descriptor, arg))
    for name, obj in syscalls.all():
        # Irregular syscalls will be handled by hand-written code elsewhere.
        if isinstance(obj, syscalls.RegularSyscall):
            f.write("  case Arch::%s:\n" % name)
            for arg in range(1,6):
                write_recorder_for_arg(obj, arg)
            f.write("    return PREVENT_SWITCH;\n")

has_syscall = string.Template("""inline bool
has_${syscall}_syscall(SupportedArch arch) {
  switch (arch) {
    case x86:
      return X86Arch::${syscall} >= 0;
    case x86_64:
      return X64Arch::${syscall} >= 0;
    default:
      DEBUG_ASSERT(0 && "unsupported architecture");
      return false;
  }
}
""")

is_syscall = string.Template("""inline bool
is_${syscall}_syscall(int syscallno, SupportedArch arch) {
  switch (arch) {
    case x86:
      return syscallno >= 0 && syscallno == X86Arch::${syscall};
    case x86_64:
      return syscallno >= 0 && syscallno == X64Arch::${syscall};
    default:
      DEBUG_ASSERT(0 && "unsupported architecture");
      return false;
  }
}
""")

syscall_number = string.Template("""inline int
syscall_number_for_${syscall}(SupportedArch arch) {
  switch (arch) {
    case x86:
      DEBUG_ASSERT(X86Arch::${syscall} >= 0);
      return X86Arch::${syscall};
    case x86_64:
      DEBUG_ASSERT(X64Arch::${syscall} >= 0);
      return X64Arch::${syscall};
    default:
      DEBUG_ASSERT(0 && "unsupported architecture");
      return -1;
  }
}
""")

def write_syscall_helper_functions(f):
    def write_helpers(syscall):
        subs = { 'syscall': syscall }
        f.write(has_syscall.safe_substitute(subs))
        f.write(is_syscall.safe_substitute(subs))
        f.write(syscall_number.safe_substitute(subs))

    for name, obj in syscalls.all():
        write_helpers(name)

def write_check_syscall_numbers(f):
    for name, obj in syscalls.all():
        # XXX hard-coded to x86 currently
        if not obj.x86:
            continue
        f.write("""static_assert(X86Arch::%s == SYS_%s, "Incorrect syscall number for %s");\n"""
                % (name, name, name))

generators_for = {
    'AssemblyTemplates': lambda f: assembly_templates.generate(f),
    'CheckSyscallNumbers': write_check_syscall_numbers,
    'syscall_consts_x86_generated': lambda f: write_syscall_consts(f, 'x86'),
    'syscall_consts_x64_generated': lambda f: write_syscall_consts(f, 'x64'),
    'syscall_consts_for_tests_x86_generated': lambda f: write_syscall_consts_for_tests(f, 'x86'),
    'syscall_consts_for_tests_x64_generated': lambda f: write_syscall_consts_for_tests(f, 'x64'),
    'syscall_name_arch_x86_generated': lambda f: write_syscallname_arch(f, 'x86'),
    'syscall_name_arch_x64_generated': lambda f: write_syscallname_arch(f, 'x64'),
    'SyscallRecordCase': write_syscall_record_cases,
    'SyscallHelperFunctions': write_syscall_helper_functions,
}

def main(argv):
    filename = argv[0]
    base, extension = os.path.splitext(os.path.basename(filename))

    if os.access(filename, os.F_OK):
        with open(filename, 'r') as f:
            before = f.read()
    else:
        before = ""

    stream = StringIO()
    generators_for[base](stream)
    after = stream.getvalue()
    stream.close()

    if before != after:
        with open(filename, 'w') as f:
            f.write(after)

if __name__ == '__main__':
    main(sys.argv[1:])
