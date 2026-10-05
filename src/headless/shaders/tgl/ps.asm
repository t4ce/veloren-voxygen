L0:
/* [0000]  */ (W)     mov (8|M0)               r12.0<1>:f    r2.0<8;8,1>:f                    {Compacted}
/* [0008]  */ (W)     mov (8|M0)               r13.0<1>:f    r4.0<8;8,1>:f                    {Compacted}
/* [0010]  */ (W)     mov (8|M0)               r0.0<1>:f     r3.0<8;8,1>:f                    {Compacted}
/* [0018]  */ (W)     mov (8|M0)               r1.0<1>:f     r5.0<8;8,1>:f                    {Compacted}
/* [0020]  */ (W)     sync.nop                             null                             {@1}
/* [0030]  */         mad (16|M0)              r2.0<1>:f     r6.3<0;0>:f       r6.1<0;0>:f       r0.0<1>:f       
/* [0040]  */         mad (16|M0)              r4.0<1>:f     r6.7<0;0>:f       r6.5<0;0>:f       r0.0<1>:f       
/* [0050]  */         mad (16|M0)              r8.0<1>:f     r7.3<0;0>:f       r7.1<0;0>:f       r0.0<1>:f       
/* [0060]  */         mad (16|M0)              r10.0<1>:f    r7.7<0;0>:f       r7.5<0;0>:f       r0.0<1>:f       
/* [0070]  */ (W)     sync.nop                             null                             {@4}
/* [0080]  */         mad (16|M0)              r126.0<1>:f   r2.0<8;1>:f       r6.0<0;0>:f       r12.0<1>:f       {Compacted}
/* [0088]  */         mad (16|M0)              r120.0<1>:f   r4.0<8;1>:f       r6.4<0;0>:f       r12.0<1>:f       {Compacted,@4}
/* [0090]  */         mad (16|M0)              r122.0<1>:f   r8.0<8;1>:f       r7.0<0;0>:f       r12.0<1>:f       {Compacted,@4}
/* [0098]  */         mad (16|M0)              r124.0<1>:f   r10.0<8;1>:f      r7.4<0;0>:f       r12.0<1>:f       {Compacted,@4}
/* [00A0]  */         sendc.rc (16|M0)         null     r126    r120    0x180            0x04031000           {EOT,@1} // wr:2+6, rd:0; full-precision render target write SIMD16; last render target to surface 0
L176:
