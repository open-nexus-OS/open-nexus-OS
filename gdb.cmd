set architecture riscv:rv64
set pagination off
file target/riscv64imac-unknown-none-elf/release/neuron-boot
target remote localhost:1234
add-symbol-file target/riscv64imac-unknown-none-elf/release/neuron-boot 0x80200000
break *0x80203e6c
continue
x/16xb 0x80203e6c
x/12i 0x80203e6c
quit
