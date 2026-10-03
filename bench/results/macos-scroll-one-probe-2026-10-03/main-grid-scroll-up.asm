
target/main-bench-20261003/loomtty.app/Contents/Helpers/loomtty-server.app/Contents/MacOS/loomtty-server:	file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001002cc998 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE>:
1002cc998: d10143ff    	sub	sp, sp, #0x50
1002cc99c: a9015ff8    	stp	x24, x23, [sp, #0x10]
1002cc9a0: a90257f6    	stp	x22, x21, [sp, #0x20]
1002cc9a4: a9034ff4    	stp	x20, x19, [sp, #0x30]
1002cc9a8: a9047bfd    	stp	x29, x30, [sp, #0x40]
1002cc9ac: 910103fd    	add	x29, sp, #0x40
1002cc9b0: aa0003f3    	mov	x19, x0
1002cc9b4: 29405428    	ldp	w8, w21, [x1]
1002cc9b8: 93407eb5    	sxtw	x21, w21
1002cc9bc: 4b0802b6    	sub	w22, w21, w8
1002cc9c0: 7100011f    	cmp	w8, #0x0
1002cc9c4: 7a4212c0    	ccmp	w22, w2, #0x0, ne
1002cc9c8: 5400052d    	b.le	0x1002cca6c <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0xd4>
1002cc9cc: f9405269    	ldr	x9, [x19, #0xa0]
1002cc9d0: b40000c9    	cbz	x9, 0x1002cc9e8 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x50>
1002cc9d4: f940566a    	ldr	x10, [x19, #0xa8]
1002cc9d8: 8b020129    	add	x9, x9, x2
1002cc9dc: eb09015f    	cmp	x10, x9
1002cc9e0: 9a893149    	csel	x9, x10, x9, lo
1002cc9e4: f9005269    	str	x9, [x19, #0xa0]
1002cc9e8: 340006e8    	cbz	w8, 0x1002ccac4 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x12c>
1002cc9ec: 4b0202aa    	sub	w10, w21, w2
1002cc9f0: 6b0a011f    	cmp	w8, w10
1002cc9f4: 5400106a    	b.ge	0x1002ccc00 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x268>
1002cc9f8: a941ae69    	ldp	x9, x11, [x19, #0x18]
1002cc9fc: d1000529    	sub	x9, x9, #0x1
1002cca00: 93407d4e    	sxtw	x14, w10
1002cca04: d3607d6d    	lsl	x13, x11, #32
1002cca08: a940aa6b    	ldp	x11, x10, [x19, #0x8]
1002cca0c: 0b08004c    	add	w12, w2, w8
1002cca10: cb0c81ac    	sub	x12, x13, x12, lsl #32
1002cca14: cb0881ad    	sub	x13, x13, x8, lsl #32
1002cca18: cb28c1c8    	sub	x8, x14, w8, sxtw
1002cca1c: b2607fee    	mov	x14, #-0x100000000      ; =-4294967296
1002cca20: 8b8d812f    	add	x15, x9, x13, asr #32
1002cca24: eb0a01ff    	cmp	x15, x10
1002cca28: 9a8a33f0    	csel	x16, xzr, x10, lo
1002cca2c: cb1001ef    	sub	x15, x15, x16
1002cca30: 8b8c8130    	add	x16, x9, x12, asr #32
1002cca34: eb0a021f    	cmp	x16, x10
1002cca38: 9a8a33f1    	csel	x17, xzr, x10, lo
1002cca3c: cb110210    	sub	x16, x16, x17
1002cca40: 8b0f156f    	add	x15, x11, x15, lsl #5
1002cca44: 8b101570    	add	x16, x11, x16, lsl #5
1002cca48: ad4005e0    	ldp	q0, q1, [x15]
1002cca4c: ad400e02    	ldp	q2, q3, [x16]
1002cca50: ad000de2    	stp	q2, q3, [x15]
1002cca54: ad000600    	stp	q0, q1, [x16]
1002cca58: 8b0e018c    	add	x12, x12, x14
1002cca5c: 8b0e01ad    	add	x13, x13, x14
1002cca60: f1000508    	subs	x8, x8, #0x1
1002cca64: 54fffde1    	b.ne	0x1002cca20 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x88>
1002cca68: 14000066    	b	0x1002ccc00 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x268>
1002cca6c: 6b0802bf    	cmp	w21, w8
1002cca70: 54000f4d    	b.le	0x1002ccc58 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x2c0>
1002cca74: a941a66a    	ldp	x10, x9, [x19, #0x18]
1002cca78: f9400a74    	ldr	x20, [x19, #0x10]
1002cca7c: d1000555    	sub	x21, x10, #0x1
1002cca80: d3607d29    	lsl	x9, x9, #32
1002cca84: cb088137    	sub	x23, x9, x8, lsl #32
1002cca88: b2607ff8    	mov	x24, #-0x100000000      ; =-4294967296
1002cca8c: 8b9782a8    	add	x8, x21, x23, asr #32
1002cca90: eb14011f    	cmp	x8, x20
1002cca94: 9a9433e9    	csel	x9, xzr, x20, lo
1002cca98: cb090100    	sub	x0, x8, x9
1002cca9c: eb14001f    	cmp	x0, x20
1002ccaa0: 54000e82    	b.hs	0x1002ccc70 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x2d8>
1002ccaa4: f9400668    	ldr	x8, [x19, #0x8]
1002ccaa8: 8b001500    	add	x0, x8, x0, lsl #5
1002ccaac: 91010261    	add	x1, x19, #0x40
1002ccab0: 9400007a    	bl	0x1002ccc98 <__ZN18alacritty_terminal4grid3row12Row$LT$T$GT$5reset17h53da9450bc7623a6E>
1002ccab4: 8b1802f7    	add	x23, x23, x24
1002ccab8: 710006d6    	subs	w22, w22, #0x1
1002ccabc: 54fffe81    	b.ne	0x1002cca8c <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0xf4>
1002ccac0: 14000066    	b	0x1002ccc58 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x2c0>
1002ccac4: f9401668    	ldr	x8, [x19, #0x28]
1002ccac8: f9404e69    	ldr	x9, [x19, #0x98]
1002ccacc: eb090109    	subs	x9, x8, x9
1002ccad0: 9a8933e9    	csel	x9, xzr, x9, lo
1002ccad4: a94aae6a    	ldp	x10, x11, [x19, #0xa8]
1002ccad8: cb090149    	sub	x9, x10, x9
1002ccadc: eb09004a    	subs	x10, x2, x9
1002ccae0: 9a8a33ea    	csel	x10, xzr, x10, lo
1002ccae4: ab0a016a    	adds	x10, x11, x10
1002ccae8: da9f314a    	csinv	x10, x10, xzr, lo
1002ccaec: f9005a6a    	str	x10, [x19, #0xb0]
1002ccaf0: eb02013f    	cmp	x9, x2
1002ccaf4: 9a823136    	csel	x22, x9, x2, lo
1002ccaf8: b4000336    	cbz	x22, 0x1002ccb5c <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x1c4>
1002ccafc: f9404a69    	ldr	x9, [x19, #0x90]
1002ccb00: f90007e9    	str	x9, [sp, #0x8]
1002ccb04: f9400a74    	ldr	x20, [x19, #0x10]
1002ccb08: 8b0802c8    	add	x8, x22, x8
1002ccb0c: eb14011f    	cmp	x8, x20
1002ccb10: 54000249    	b.ls	0x1002ccb58 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x1c0>
1002ccb14: aa0203f7    	mov	x23, x2
1002ccb18: f9400e62    	ldr	x2, [x19, #0x18]
1002ccb1c: b40000a2    	cbz	x2, 0x1002ccb30 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x198>
1002ccb20: f9400660    	ldr	x0, [x19, #0x8]
1002ccb24: aa1403e1    	mov	x1, x20
1002ccb28: 94004646    	bl	0x1002de440 <__ZN4core5slice29_$LT$impl$u20$$u5b$T$u5d$$GT$11rotate_left17h31e6037afffd0bd6E>
1002ccb2c: f9000e7f    	str	xzr, [x19, #0x18]
1002ccb30: 52807d08    	mov	w8, #0x3e8              ; =1000
1002ccb34: f10fa2df    	cmp	x22, #0x3e8
1002ccb38: 9a8882c8    	csel	x8, x22, x8, hi
1002ccb3c: 8b080281    	add	x1, x20, x8
1002ccb40: 910023e2    	add	x2, sp, #0x8
1002ccb44: aa1303e0    	mov	x0, x19
1002ccb48: 94003f04    	bl	0x1002dc758 <__ZN5alloc3vec16Vec$LT$T$C$A$GT$11resize_with17hd65ccb275697318eE>
1002ccb4c: f9401668    	ldr	x8, [x19, #0x28]
1002ccb50: 8b160108    	add	x8, x8, x22
1002ccb54: aa1703e2    	mov	x2, x23
1002ccb58: f9001668    	str	x8, [x19, #0x28]
1002ccb5c: f9400a68    	ldr	x8, [x19, #0x10]
1002ccb60: b4000968    	cbz	x8, 0x1002ccc8c <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x2f4>
1002ccb64: f9400e69    	ldr	x9, [x19, #0x18]
1002ccb68: cb02010a    	sub	x10, x8, x2
1002ccb6c: 8b090149    	add	x9, x10, x9
1002ccb70: 9ac8092a    	udiv	x10, x9, x8
1002ccb74: 9b08a549    	msub	x9, x10, x8, x9
1002ccb78: f9000e69    	str	x9, [x19, #0x18]
1002ccb7c: f9404e6e    	ldr	x14, [x19, #0x98]
1002ccb80: 6b0e02bf    	cmp	w21, w14
1002ccb84: 540003ea    	b.ge	0x1002ccc00 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x268>
1002ccb88: b940226f    	ldr	w15, [x19, #0x20]
1002ccb8c: d1000529    	sub	x9, x9, #0x1
1002ccb90: f940066a    	ldr	x10, [x19, #0x8]
1002ccb94: 93407dcb    	sxtw	x11, w14
1002ccb98: 0b0201ec    	add	w12, w15, w2
1002ccb9c: 4b0e018d    	sub	w13, w12, w14
1002ccba0: d2c0002c    	mov	x12, #0x100000000       ; =4294967296
1002ccba4: 8b0d818d    	add	x13, x12, x13, lsl #32
1002ccba8: 4b0e01ee    	sub	w14, w15, w14
1002ccbac: 110005ce    	add	w14, w14, #0x1
1002ccbb0: d3607dce    	lsl	x14, x14, #32
1002ccbb4: 8b8e812f    	add	x15, x9, x14, asr #32
1002ccbb8: eb0801ff    	cmp	x15, x8
1002ccbbc: 9a8833f0    	csel	x16, xzr, x8, lo
1002ccbc0: cb1001ef    	sub	x15, x15, x16
1002ccbc4: 8b8d8130    	add	x16, x9, x13, asr #32
1002ccbc8: eb08021f    	cmp	x16, x8
1002ccbcc: 9a8833f1    	csel	x17, xzr, x8, lo
1002ccbd0: cb110210    	sub	x16, x16, x17
1002ccbd4: 8b0f154f    	add	x15, x10, x15, lsl #5
1002ccbd8: 8b101550    	add	x16, x10, x16, lsl #5
1002ccbdc: ad4005e0    	ldp	q0, q1, [x15]
1002ccbe0: ad400e02    	ldp	q2, q3, [x16]
1002ccbe4: ad000de2    	stp	q2, q3, [x15]
1002ccbe8: d100056b    	sub	x11, x11, #0x1
1002ccbec: ad000600    	stp	q0, q1, [x16]
1002ccbf0: 8b0c01ad    	add	x13, x13, x12
1002ccbf4: 8b0c01ce    	add	x14, x14, x12
1002ccbf8: eb15017f    	cmp	x11, x21
1002ccbfc: 54fffdcc    	b.gt	0x1002ccbb4 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x21c>
1002ccc00: 4b0202a8    	sub	w8, w21, w2
1002ccc04: 6b15011f    	cmp	w8, w21
1002ccc08: 5400028a    	b.ge	0x1002ccc58 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x2c0>
1002ccc0c: 4b0203f4    	neg	w20, w2
1002ccc10: 4b150055    	sub	w21, w2, w21
1002ccc14: b9402268    	ldr	w8, [x19, #0x20]
1002ccc18: 0b0802a8    	add	w8, w21, w8
1002ccc1c: a9412661    	ldp	x1, x9, [x19, #0x10]
1002ccc20: 8b28c128    	add	x8, x9, w8, sxtw
1002ccc24: d1000508    	sub	x8, x8, #0x1
1002ccc28: eb01011f    	cmp	x8, x1
1002ccc2c: 9a8133e9    	csel	x9, xzr, x1, lo
1002ccc30: cb090100    	sub	x0, x8, x9
1002ccc34: eb01001f    	cmp	x0, x1
1002ccc38: 54000242    	b.hs	0x1002ccc80 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x2e8>
1002ccc3c: f9400668    	ldr	x8, [x19, #0x8]
1002ccc40: 8b001500    	add	x0, x8, x0, lsl #5
1002ccc44: 91010261    	add	x1, x19, #0x40
1002ccc48: 94000014    	bl	0x1002ccc98 <__ZN18alacritty_terminal4grid3row12Row$LT$T$GT$5reset17h53da9450bc7623a6E>
1002ccc4c: 510006b5    	sub	w21, w21, #0x1
1002ccc50: 31000694    	adds	w20, w20, #0x1
1002ccc54: 54fffe03    	b.lo	0x1002ccc14 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17h271cf101757d8a4dE+0x27c>
1002ccc58: a9447bfd    	ldp	x29, x30, [sp, #0x40]
1002ccc5c: a9434ff4    	ldp	x20, x19, [sp, #0x30]
1002ccc60: a94257f6    	ldp	x22, x21, [sp, #0x20]
1002ccc64: a9415ff8    	ldp	x24, x23, [sp, #0x10]
1002ccc68: 910143ff    	add	sp, sp, #0x50
1002ccc6c: d65f03c0    	ret
1002ccc70: b0002202    	adrp	x2, 0x10070d000 <_anon.6040802536097db4fcb033e2e09bcdf4.53+0x1010>
1002ccc74: 91340042    	add	x2, x2, #0xd00
1002ccc78: aa1403e1    	mov	x1, x20
1002ccc7c: 940922cf    	bl	0x1005157b8 <__ZN4core9panicking18panic_bounds_check17h4d34bdab34d851beE>
1002ccc80: b0002202    	adrp	x2, 0x10070d000 <_anon.6040802536097db4fcb033e2e09bcdf4.53+0x1010>
1002ccc84: 9133a042    	add	x2, x2, #0xce8
1002ccc88: 940922cc    	bl	0x1005157b8 <__ZN4core9panicking18panic_bounds_check17h4d34bdab34d851beE>
1002ccc8c: b0002200    	adrp	x0, 0x10070d000 <_anon.6040802536097db4fcb033e2e09bcdf4.53+0x1010>
1002ccc90: 91334000    	add	x0, x0, #0xcd0
1002ccc94: 94092288    	bl	0x1005156b4 <__ZN4core9panicking11panic_const23panic_const_rem_by_zero17h0a5afac2d947308bE>
