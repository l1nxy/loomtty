
target/scroll-server-ab-v3-20261003/loomtty-server.app/Contents/MacOS/loomtty-server:	file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001002afe0c <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E>:
1002afe0c: d10443ff    	sub	sp, sp, #0x110
1002afe10: a90d6ffc    	stp	x28, x27, [sp, #0xd0]
1002afe14: a90e57f6    	stp	x22, x21, [sp, #0xe0]
1002afe18: a90f4ff4    	stp	x20, x19, [sp, #0xf0]
1002afe1c: a9107bfd    	stp	x29, x30, [sp, #0x100]
1002afe20: 910403fd    	add	x29, sp, #0x100
1002afe24: aa0103f4    	mov	x20, x1
1002afe28: aa0003f3    	mov	x19, x0
1002afe2c: b9000fe1    	str	w1, [sp, #0xc]
1002afe30: f9000be2    	str	x2, [sp, #0x10]
1002afe34: b0002548    	adrp	x8, 0x100758000 <__ZN15crossbeam_utils6atomic11atomic_cell4lock5LOCKS17hbe4d84cae547c90aE+0x2080>
1002afe38: 91042108    	add	x8, x8, #0x108
1002afe3c: f9400108    	ldr	x8, [x8]
1002afe40: f100111f    	cmp	x8, #0x4
1002afe44: 54000489    	b.ls	0x1002afed4 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0xc8>
1002afe48: 90000248    	adrp	x8, 0x1002f7000 <__ZN75_$LT$alacritty_terminal..term..Config$u20$as$u20$core..default..Default$GT$7default17h78e9ebca0f0003d7E+0x8>
1002afe4c: 9132b108    	add	x8, x8, #0xcac
1002afe50: 9101a3e9    	add	x9, sp, #0x68
1002afe54: 910033ea    	add	x10, sp, #0xc
1002afe58: a901a3ea    	stp	x10, x8, [sp, #0x18]
1002afe5c: 910043e8    	add	x8, sp, #0x10
1002afe60: 90000fca    	adrp	x10, 0x1004a7000 <__ZN4core3fmt3num3imp21_$LT$impl$u20$u32$GT$10_fmt_inner17h5b2b8fa1eacc0bcfE+0x30>
1002afe64: 9123c14a    	add	x10, x10, #0x8f0
1002afe68: a902abe8    	stp	x8, x10, [sp, #0x28]
1002afe6c: 90001688    	adrp	x8, 0x10057f000 <__ZN4zmij18POW10_SIGNIFICANDS17hf08c535f6828aa50E+0x2080>
1002afe70: 912fb908    	add	x8, x8, #0xbee
1002afe74: 5280030a    	mov	w10, #0x18              ; =24
1002afe78: 90001a4b    	adrp	x11, 0x1005f7000 <_anon.d3a04026ab60a309fe2224831479a208.42+0x2a>
1002afe7c: 9128e96b    	add	x11, x11, #0xa3a
1002afe80: a903a928    	stp	x8, x10, [x9, #0x38]
1002afe84: 910063ea    	add	x10, sp, #0x18
1002afe88: a905292b    	stp	x11, x10, [x9, #0x50]
1002afe8c: a906a3ff    	stp	xzr, x8, [sp, #0x68]
1002afe90: b0001628    	adrp	x8, 0x100574000 <__ZN8httparse9TOKEN_MAP17hb1ecc0354bded310E+0xf0>
1002afe94: 3dc09500    	ldr	q0, [x8, #0x250]
1002afe98: 3c8783e0    	stur	q0, [sp, #0x78]
1002afe9c: 90001a48    	adrp	x8, 0x1005f7000 <_anon.d3a04026ab60a309fe2224831479a208.42+0x2a>
1002afea0: 91273108    	add	x8, x8, #0x9cc
1002afea4: f90047e8    	str	x8, [sp, #0x88]
1002afea8: 90001688    	adrp	x8, 0x10057f000 <__ZN4zmij18POW10_SIGNIFICANDS17hf08c535f6828aa50E+0x2080>
1002afeac: 3dc1c500    	ldr	q0, [x8, #0x710]
1002afeb0: 3c828120    	stur	q0, [x9, #0x28]
1002afeb4: 90001688    	adrp	x8, 0x10057f000 <__ZN4zmij18POW10_SIGNIFICANDS17hf08c535f6828aa50E+0x2080>
1002afeb8: fd441900    	ldr	d0, [x8, #0x830]
1002afebc: fd002520    	str	d0, [x9, #0x48]
1002afec0: d100c7a0    	sub	x0, x29, #0x31
1002afec4: 9101a3e1    	add	x1, sp, #0x68
1002afec8: 94062cca    	bl	0x10043b1f0 <__ZN61_$LT$log..__private_api..GlobalLogger$u20$as$u20$log..Log$GT$3log17h2dbab5f7fea7620dE>
1002afecc: f9400be2    	ldr	x2, [sp, #0x10]
1002afed0: b9400ff4    	ldr	w20, [sp, #0xc]
1002afed4: b9829e75    	ldrsw	x21, [x19, #0x29c]
1002afed8: b9429a68    	ldr	w8, [x19, #0x298]
1002afedc: 4b0802a8    	sub	w8, w21, w8
1002afee0: 93407d08    	sxtw	x8, w8
1002afee4: eb02011f    	cmp	x8, x2
1002afee8: 9a823102    	csel	x2, x8, x2, lo
1002afeec: f9000be2    	str	x2, [sp, #0x10]
1002afef0: 290d57f4    	stp	w20, w21, [sp, #0x68]
1002afef4: f9412a6e    	ldr	x14, [x19, #0x250]
1002afef8: b9425a61    	ldr	w1, [x19, #0x258]
1002afefc: b9425e6f    	ldr	w15, [x19, #0x25c]
1002aff00: 39498270    	ldrb	w16, [x19, #0x260]
1002aff04: 91098668    	add	x8, x19, #0x261
1002aff08: b9400109    	ldr	w9, [x8]
1002aff0c: b9004be9    	str	w9, [sp, #0x48]
1002aff10: b9426669    	ldr	w9, [x19, #0x264]
1002aff14: b804b3e9    	stur	w9, [sp, #0x4b]
1002aff18: f9413660    	ldr	x0, [x19, #0x268]
1002aff1c: b9427263    	ldr	w3, [x19, #0x270]
1002aff20: b9427671    	ldr	w17, [x19, #0x274]
1002aff24: 3949e26d    	ldrb	w13, [x19, #0x278]
1002aff28: 9109e669    	add	x9, x19, #0x279
1002aff2c: b940012a    	ldr	w10, [x9]
1002aff30: b9427e6b    	ldr	w11, [x19, #0x27c]
1002aff34: b80433eb    	stur	w11, [sp, #0x43]
1002aff38: b90043ea    	str	w10, [sp, #0x40]
1002aff3c: 394a026b    	ldrb	w11, [x19, #0x280]
1002aff40: 910a066a    	add	x10, x19, #0x281
1002aff44: b940014c    	ldr	w12, [x10]
1002aff48: b9428664    	ldr	w4, [x19, #0x284]
1002aff4c: b803b3e4    	stur	w4, [sp, #0x3b]
1002aff50: b9003bec    	str	w12, [sp, #0x38]
1002aff54: 710009bf    	cmp	w13, #0x2
1002aff58: 54000081    	b.ne	0x1002aff68 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x15c>
1002aff5c: 5280000b    	mov	w11, #0x0               ; =0
1002aff60: f940566c    	ldr	x12, [x19, #0xa8]
1002aff64: 14000037    	b	0x1002b0040 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x234>
1002aff68: f940566c    	ldr	x12, [x19, #0xa8]
1002aff6c: 51000585    	sub	w5, w12, #0x1
1002aff70: eb0001df    	cmp	x14, x0
1002aff74: 1a9f97e4    	cset	w4, hi
1002aff78: 6b03003f    	cmp	w1, w3
1002aff7c: 1a9fd7e6    	cset	w6, gt
1002aff80: 1a860084    	csel	w4, w4, w6, eq
1002aff84: 7100009f    	cmp	w4, #0x0
1002aff88: 1a811066    	csel	w6, w3, w1, ne
1002aff8c: 7100029f    	cmp	w20, #0x0
1002aff90: 7a5410c0    	ccmp	w6, w20, #0x0, ne
1002aff94: 1a9fb7e7    	cset	w7, ge
1002aff98: 6b1500df    	cmp	w6, w21
1002aff9c: 5400034a    	b.ge	0x1002b0004 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
1002affa0: 36000327    	tbz	w7, #0x0, 0x1002b0004 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
1002affa4: 4b0200c6    	sub	w6, w6, w2
1002affa8: 6b0600bf    	cmp	w5, w6
1002affac: 1a86b0a6    	csel	w6, w5, w6, lt
1002affb0: 7100009f    	cmp	w4, #0x0
1002affb4: 1a831027    	csel	w7, w1, w3, ne
1002affb8: 6b1500df    	cmp	w6, w21
1002affbc: 5400006b    	b.lt	0x1002affc8 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1bc>
1002affc0: 6b1500ff    	cmp	w7, w21
1002affc4: 540003ab    	b.lt	0x1002b0038 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x22c>
1002affc8: 7100009f    	cmp	w4, #0x0
1002affcc: 1a861027    	csel	w7, w1, w6, ne
1002affd0: 1a8310d6    	csel	w22, w6, w3, ne
1002affd4: 34000154    	cbz	w20, 0x1002afffc <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f0>
1002affd8: 6b1400df    	cmp	w6, w20
1002affdc: 5400010a    	b.ge	0x1002afffc <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f0>
1002affe0: 7100057f    	cmp	w11, #0x1
1002affe4: 540004e1    	b.ne	0x1002b0080 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x274>
1002affe8: 7100009f    	cmp	w4, #0x0
1002affec: 1a831283    	csel	w3, w20, w3, ne
1002afff0: 1a941021    	csel	w1, w1, w20, ne
1002afff4: aa1403e6    	mov	x6, x20
1002afff8: 14000003    	b	0x1002b0004 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
1002afffc: aa1603e3    	mov	x3, x22
1002b0000: aa0703e1    	mov	x1, x7
1002b0004: 7100009f    	cmp	w4, #0x0
1002b0008: 1a831027    	csel	w7, w1, w3, ne
1002b000c: 7100029f    	cmp	w20, #0x0
1002b0010: 7a5410e0    	ccmp	w7, w20, #0x0, ne
1002b0014: 1a9fb7f6    	cset	w22, ge
1002b0018: 6b1500ff    	cmp	w7, w21
1002b001c: 5400052a    	b.ge	0x1002b00c0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2b4>
1002b0020: 36000516    	tbz	w22, #0x0, 0x1002b00c0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2b4>
1002b0024: 4b0200e7    	sub	w7, w7, w2
1002b0028: 6b0700bf    	cmp	w5, w7
1002b002c: 1a87b0a7    	csel	w7, w5, w7, lt
1002b0030: 6b0600ff    	cmp	w7, w6
1002b0034: 5400008a    	b.ge	0x1002b0044 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x238>
1002b0038: 5280000b    	mov	w11, #0x0               ; =0
1002b003c: 5280004d    	mov	w13, #0x2               ; =2
1002b0040: 1400002c    	b	0x1002b00f0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2e4>
1002b0044: 7100009f    	cmp	w4, #0x0
1002b0048: 1a8110e5    	csel	w5, w7, w1, ne
1002b004c: 1a871066    	csel	w6, w3, w7, ne
1002b0050: 6b1500ff    	cmp	w7, w21
1002b0054: 5400008a    	b.ge	0x1002b0064 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x258>
1002b0058: aa0603e3    	mov	x3, x6
1002b005c: aa0503e1    	mov	x1, x5
1002b0060: 14000018    	b	0x1002b00c0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2b4>
1002b0064: 7100057f    	cmp	w11, #0x1
1002b0068: 54000181    	b.ne	0x1002b0098 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x28c>
1002b006c: 510006a5    	sub	w5, w21, #0x1
1002b0070: 7100009f    	cmp	w4, #0x0
1002b0074: 1a851063    	csel	w3, w3, w5, ne
1002b0078: 1a8110a1    	csel	w1, w5, w1, ne
1002b007c: 14000011    	b	0x1002b00c0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2b4>
1002b0080: 36000a44    	tbz	w4, #0x0, 0x1002b01c8 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x3bc>
1002b0084: d2800000    	mov	x0, #0x0                ; =0
1002b0088: 5280000d    	mov	w13, #0x0               ; =0
1002b008c: aa1403e3    	mov	x3, x20
1002b0090: aa1403e6    	mov	x6, x20
1002b0094: 17ffffdc    	b	0x1002b0004 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
1002b0098: f9405265    	ldr	x5, [x19, #0xa0]
1002b009c: d10004a5    	sub	x5, x5, #0x1
1002b00a0: 510006a6    	sub	w6, w21, #0x1
1002b00a4: 7100009f    	cmp	w4, #0x0
1002b00a8: 1a861063    	csel	w3, w3, w6, ne
1002b00ac: 1a9f0610    	csinc	w16, w16, wzr, eq
1002b00b0: 9a851000    	csel	x0, x0, x5, ne
1002b00b4: 1a9f15ad    	csinc	w13, w13, wzr, ne
1002b00b8: 1a8110c1    	csel	w1, w6, w1, ne
1002b00bc: 9a8e10ae    	csel	x14, x5, x14, ne
1002b00c0: b9404be4    	ldr	w4, [sp, #0x48]
1002b00c4: b90063e4    	str	w4, [sp, #0x60]
1002b00c8: b844b3e4    	ldur	w4, [sp, #0x4b]
1002b00cc: b80633e4    	stur	w4, [sp, #0x63]
1002b00d0: b94043e4    	ldr	w4, [sp, #0x40]
1002b00d4: b9005be4    	str	w4, [sp, #0x58]
1002b00d8: b84433e4    	ldur	w4, [sp, #0x43]
1002b00dc: b805b3e4    	stur	w4, [sp, #0x5b]
1002b00e0: b9403be4    	ldr	w4, [sp, #0x38]
1002b00e4: b90053e4    	str	w4, [sp, #0x50]
1002b00e8: b843b3e4    	ldur	w4, [sp, #0x3b]
1002b00ec: b80533e4    	stur	w4, [sp, #0x53]
1002b00f0: f9012a6e    	str	x14, [x19, #0x250]
1002b00f4: b9025a61    	str	w1, [x19, #0x258]
1002b00f8: b9025e6f    	str	w15, [x19, #0x25c]
1002b00fc: 39098270    	strb	w16, [x19, #0x260]
1002b0100: b94063ee    	ldr	w14, [sp, #0x60]
1002b0104: b900010e    	str	w14, [x8]
1002b0108: b84633ee    	ldur	w14, [sp, #0x63]
1002b010c: b800310e    	stur	w14, [x8, #0x3]
1002b0110: f9013660    	str	x0, [x19, #0x268]
1002b0114: b9027263    	str	w3, [x19, #0x270]
1002b0118: b9027671    	str	w17, [x19, #0x274]
1002b011c: 3909e26d    	strb	w13, [x19, #0x278]
1002b0120: b9405be8    	ldr	w8, [sp, #0x58]
1002b0124: b9000128    	str	w8, [x9]
1002b0128: b845b3e8    	ldur	w8, [sp, #0x5b]
1002b012c: b8003128    	stur	w8, [x9, #0x3]
1002b0130: 390a026b    	strb	w11, [x19, #0x280]
1002b0134: b94053e8    	ldr	w8, [sp, #0x50]
1002b0138: b84533e9    	ldur	w9, [sp, #0x53]
1002b013c: b8003149    	stur	w9, [x10, #0x3]
1002b0140: b9000148    	str	w8, [x10]
1002b0144: f100045f    	cmp	x2, #0x1
1002b0148: 540000e1    	b.ne	0x1002b0164 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x358>
1002b014c: 350000d4    	cbnz	w20, 0x1002b0164 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x358>
1002b0150: eb15019f    	cmp	x12, x21
1002b0154: 54000081    	b.ne	0x1002b0164 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x358>
1002b0158: 91004260    	add	x0, x19, #0x10
1002b015c: 9400bd9b    	bl	0x1002df7c8 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$18scroll_up_full_one17h516f76671bd0d16fE>
1002b0160: 14000004    	b	0x1002b0170 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x364>
1002b0164: 91004260    	add	x0, x19, #0x10
1002b0168: 9101a3e1    	add	x1, sp, #0x68
1002b016c: 9400beaf    	bl	0x1002dfc28 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17he403fc1484f38229E>
1002b0170: b940b268    	ldr	w8, [x19, #0xb0]
1002b0174: b9400fe9    	ldr	w9, [sp, #0xc]
1002b0178: 7100029f    	cmp	w20, #0x0
1002b017c: 5a881528    	csneg	w8, w9, w8, ne
1002b0180: b9429269    	ldr	w9, [x19, #0x290]
1002b0184: 6b09011f    	cmp	w8, w9
1002b0188: 5400010c    	b.gt	0x1002b01a8 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x39c>
1002b018c: 6b0902bf    	cmp	w21, w9
1002b0190: 540000cd    	b.le	0x1002b01a8 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x39c>
1002b0194: b94013ea    	ldr	w10, [sp, #0x10]
1002b0198: 4b0a0129    	sub	w9, w9, w10
1002b019c: 6b09011f    	cmp	w8, w9
1002b01a0: 1a89c108    	csel	w8, w8, w9, gt
1002b01a4: b9029268    	str	w8, [x19, #0x290]
1002b01a8: 52800028    	mov	w8, #0x1                ; =1
1002b01ac: 39082268    	strb	w8, [x19, #0x208]
1002b01b0: a9507bfd    	ldp	x29, x30, [sp, #0x100]
1002b01b4: a94f4ff4    	ldp	x20, x19, [sp, #0xf0]
1002b01b8: a94e57f6    	ldp	x22, x21, [sp, #0xe0]
1002b01bc: a94d6ffc    	ldp	x28, x27, [sp, #0xd0]
1002b01c0: 910443ff    	add	sp, sp, #0x110
1002b01c4: d65f03c0    	ret
1002b01c8: 52800010    	mov	w16, #0x0               ; =0
1002b01cc: d280000e    	mov	x14, #0x0               ; =0
1002b01d0: aa1403e1    	mov	x1, x20
1002b01d4: aa1403e6    	mov	x6, x20
1002b01d8: 17ffff8b    	b	0x1002b0004 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>

00000001002bf098 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E>:
1002bf098: d10443ff    	sub	sp, sp, #0x110
1002bf09c: a90d6ffc    	stp	x28, x27, [sp, #0xd0]
1002bf0a0: a90e57f6    	stp	x22, x21, [sp, #0xe0]
1002bf0a4: a90f4ff4    	stp	x20, x19, [sp, #0xf0]
1002bf0a8: a9107bfd    	stp	x29, x30, [sp, #0x100]
1002bf0ac: 910403fd    	add	x29, sp, #0x100
1002bf0b0: aa0103f4    	mov	x20, x1
1002bf0b4: aa0003f3    	mov	x19, x0
1002bf0b8: b9000fe1    	str	w1, [sp, #0xc]
1002bf0bc: f9000be2    	str	x2, [sp, #0x10]
1002bf0c0: b00024c8    	adrp	x8, 0x100758000 <__ZN15crossbeam_utils6atomic11atomic_cell4lock5LOCKS17hbe4d84cae547c90aE+0x2080>
1002bf0c4: 91042108    	add	x8, x8, #0x108
1002bf0c8: f9400108    	ldr	x8, [x8]
1002bf0cc: f100111f    	cmp	x8, #0x4
1002bf0d0: 54000489    	b.ls	0x1002bf160 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0xc8>
1002bf0d4: 900001c8    	adrp	x8, 0x1002f7000 <__ZN75_$LT$alacritty_terminal..term..Config$u20$as$u20$core..default..Default$GT$7default17h78e9ebca0f0003d7E+0x8>
1002bf0d8: 9132b108    	add	x8, x8, #0xcac
1002bf0dc: 9101a3e9    	add	x9, sp, #0x68
1002bf0e0: 910033ea    	add	x10, sp, #0xc
1002bf0e4: a901a3ea    	stp	x10, x8, [sp, #0x18]
1002bf0e8: 910043e8    	add	x8, sp, #0x10
1002bf0ec: 90000f4a    	adrp	x10, 0x1004a7000 <__ZN4core3fmt3num3imp21_$LT$impl$u20$u32$GT$10_fmt_inner17h5b2b8fa1eacc0bcfE+0x30>
1002bf0f0: 9123c14a    	add	x10, x10, #0x8f0
1002bf0f4: a902abe8    	stp	x8, x10, [sp, #0x28]
1002bf0f8: b0001608    	adrp	x8, 0x100580000 <__ZN4zmij7DIGITS217hd2fe0e3e5821e82cE+0x9c0>
1002bf0fc: 91134508    	add	x8, x8, #0x4d1
1002bf100: 5280030a    	mov	w10, #0x18              ; =24
1002bf104: 900019cb    	adrp	x11, 0x1005f7000 <_anon.d3a04026ab60a309fe2224831479a208.42+0x2a>
1002bf108: 9128e96b    	add	x11, x11, #0xa3a
1002bf10c: a903a928    	stp	x8, x10, [x9, #0x38]
1002bf110: 910063ea    	add	x10, sp, #0x18
1002bf114: a905292b    	stp	x11, x10, [x9, #0x50]
1002bf118: a906a3ff    	stp	xzr, x8, [sp, #0x68]
1002bf11c: b00015a8    	adrp	x8, 0x100574000 <__ZN8httparse9TOKEN_MAP17hb1ecc0354bded310E+0xf0>
1002bf120: 3dc09500    	ldr	q0, [x8, #0x250]
1002bf124: 3c8783e0    	stur	q0, [sp, #0x78]
1002bf128: 900019c8    	adrp	x8, 0x1005f7000 <_anon.d3a04026ab60a309fe2224831479a208.42+0x2a>
1002bf12c: 91273108    	add	x8, x8, #0x9cc
1002bf130: f90047e8    	str	x8, [sp, #0x88]
1002bf134: 90001608    	adrp	x8, 0x10057f000 <__ZN4zmij18POW10_SIGNIFICANDS17hf08c535f6828aa50E+0x2080>
1002bf138: 3dc1c500    	ldr	q0, [x8, #0x710]
1002bf13c: 3c828120    	stur	q0, [x9, #0x28]
1002bf140: 90001608    	adrp	x8, 0x10057f000 <__ZN4zmij18POW10_SIGNIFICANDS17hf08c535f6828aa50E+0x2080>
1002bf144: fd441900    	ldr	d0, [x8, #0x830]
1002bf148: fd002520    	str	d0, [x9, #0x48]
1002bf14c: d100c7a0    	sub	x0, x29, #0x31
1002bf150: 9101a3e1    	add	x1, sp, #0x68
1002bf154: 9405f027    	bl	0x10043b1f0 <__ZN61_$LT$log..__private_api..GlobalLogger$u20$as$u20$log..Log$GT$3log17h2dbab5f7fea7620dE>
1002bf158: f9400be2    	ldr	x2, [sp, #0x10]
1002bf15c: b9400ff4    	ldr	w20, [sp, #0xc]
1002bf160: b9829e75    	ldrsw	x21, [x19, #0x29c]
1002bf164: b9429a68    	ldr	w8, [x19, #0x298]
1002bf168: 4b0802a8    	sub	w8, w21, w8
1002bf16c: 93407d08    	sxtw	x8, w8
1002bf170: eb02011f    	cmp	x8, x2
1002bf174: 9a823102    	csel	x2, x8, x2, lo
1002bf178: f9000be2    	str	x2, [sp, #0x10]
1002bf17c: 290d57f4    	stp	w20, w21, [sp, #0x68]
1002bf180: f9412a6e    	ldr	x14, [x19, #0x250]
1002bf184: b9425a61    	ldr	w1, [x19, #0x258]
1002bf188: b9425e6f    	ldr	w15, [x19, #0x25c]
1002bf18c: 39498270    	ldrb	w16, [x19, #0x260]
1002bf190: 91098668    	add	x8, x19, #0x261
1002bf194: b9400109    	ldr	w9, [x8]
1002bf198: b9004be9    	str	w9, [sp, #0x48]
1002bf19c: b9426669    	ldr	w9, [x19, #0x264]
1002bf1a0: b804b3e9    	stur	w9, [sp, #0x4b]
1002bf1a4: f9413660    	ldr	x0, [x19, #0x268]
1002bf1a8: b9427263    	ldr	w3, [x19, #0x270]
1002bf1ac: b9427671    	ldr	w17, [x19, #0x274]
1002bf1b0: 3949e26d    	ldrb	w13, [x19, #0x278]
1002bf1b4: 9109e669    	add	x9, x19, #0x279
1002bf1b8: b940012a    	ldr	w10, [x9]
1002bf1bc: b9427e6b    	ldr	w11, [x19, #0x27c]
1002bf1c0: b80433eb    	stur	w11, [sp, #0x43]
1002bf1c4: b90043ea    	str	w10, [sp, #0x40]
1002bf1c8: 394a026b    	ldrb	w11, [x19, #0x280]
1002bf1cc: 910a066a    	add	x10, x19, #0x281
1002bf1d0: b940014c    	ldr	w12, [x10]
1002bf1d4: b9428664    	ldr	w4, [x19, #0x284]
1002bf1d8: b803b3e4    	stur	w4, [sp, #0x3b]
1002bf1dc: b9003bec    	str	w12, [sp, #0x38]
1002bf1e0: 710009bf    	cmp	w13, #0x2
1002bf1e4: 54000081    	b.ne	0x1002bf1f4 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x15c>
1002bf1e8: 5280000b    	mov	w11, #0x0               ; =0
1002bf1ec: f940566c    	ldr	x12, [x19, #0xa8]
1002bf1f0: 14000037    	b	0x1002bf2cc <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x234>
1002bf1f4: f940566c    	ldr	x12, [x19, #0xa8]
1002bf1f8: 51000585    	sub	w5, w12, #0x1
1002bf1fc: eb0001df    	cmp	x14, x0
1002bf200: 1a9f97e4    	cset	w4, hi
1002bf204: 6b03003f    	cmp	w1, w3
1002bf208: 1a9fd7e6    	cset	w6, gt
1002bf20c: 1a860084    	csel	w4, w4, w6, eq
1002bf210: 7100009f    	cmp	w4, #0x0
1002bf214: 1a811066    	csel	w6, w3, w1, ne
1002bf218: 7100029f    	cmp	w20, #0x0
1002bf21c: 7a5410c0    	ccmp	w6, w20, #0x0, ne
1002bf220: 1a9fb7e7    	cset	w7, ge
1002bf224: 6b1500df    	cmp	w6, w21
1002bf228: 5400034a    	b.ge	0x1002bf290 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
1002bf22c: 36000327    	tbz	w7, #0x0, 0x1002bf290 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
1002bf230: 4b0200c6    	sub	w6, w6, w2
1002bf234: 6b0600bf    	cmp	w5, w6
1002bf238: 1a86b0a6    	csel	w6, w5, w6, lt
1002bf23c: 7100009f    	cmp	w4, #0x0
1002bf240: 1a831027    	csel	w7, w1, w3, ne
1002bf244: 6b1500df    	cmp	w6, w21
1002bf248: 5400006b    	b.lt	0x1002bf254 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1bc>
1002bf24c: 6b1500ff    	cmp	w7, w21
1002bf250: 540003ab    	b.lt	0x1002bf2c4 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x22c>
1002bf254: 7100009f    	cmp	w4, #0x0
1002bf258: 1a861027    	csel	w7, w1, w6, ne
1002bf25c: 1a8310d6    	csel	w22, w6, w3, ne
1002bf260: 34000154    	cbz	w20, 0x1002bf288 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f0>
1002bf264: 6b1400df    	cmp	w6, w20
1002bf268: 5400010a    	b.ge	0x1002bf288 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f0>
1002bf26c: 7100057f    	cmp	w11, #0x1
1002bf270: 540004e1    	b.ne	0x1002bf30c <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x274>
1002bf274: 7100009f    	cmp	w4, #0x0
1002bf278: 1a831283    	csel	w3, w20, w3, ne
1002bf27c: 1a941021    	csel	w1, w1, w20, ne
1002bf280: aa1403e6    	mov	x6, x20
1002bf284: 14000003    	b	0x1002bf290 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
1002bf288: aa1603e3    	mov	x3, x22
1002bf28c: aa0703e1    	mov	x1, x7
1002bf290: 7100009f    	cmp	w4, #0x0
1002bf294: 1a831027    	csel	w7, w1, w3, ne
1002bf298: 7100029f    	cmp	w20, #0x0
1002bf29c: 7a5410e0    	ccmp	w7, w20, #0x0, ne
1002bf2a0: 1a9fb7f6    	cset	w22, ge
1002bf2a4: 6b1500ff    	cmp	w7, w21
1002bf2a8: 5400052a    	b.ge	0x1002bf34c <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2b4>
1002bf2ac: 36000516    	tbz	w22, #0x0, 0x1002bf34c <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2b4>
1002bf2b0: 4b0200e7    	sub	w7, w7, w2
1002bf2b4: 6b0700bf    	cmp	w5, w7
1002bf2b8: 1a87b0a7    	csel	w7, w5, w7, lt
1002bf2bc: 6b0600ff    	cmp	w7, w6
1002bf2c0: 5400008a    	b.ge	0x1002bf2d0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x238>
1002bf2c4: 5280000b    	mov	w11, #0x0               ; =0
1002bf2c8: 5280004d    	mov	w13, #0x2               ; =2
1002bf2cc: 1400002c    	b	0x1002bf37c <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2e4>
1002bf2d0: 7100009f    	cmp	w4, #0x0
1002bf2d4: 1a8110e5    	csel	w5, w7, w1, ne
1002bf2d8: 1a871066    	csel	w6, w3, w7, ne
1002bf2dc: 6b1500ff    	cmp	w7, w21
1002bf2e0: 5400008a    	b.ge	0x1002bf2f0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x258>
1002bf2e4: aa0603e3    	mov	x3, x6
1002bf2e8: aa0503e1    	mov	x1, x5
1002bf2ec: 14000018    	b	0x1002bf34c <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2b4>
1002bf2f0: 7100057f    	cmp	w11, #0x1
1002bf2f4: 54000181    	b.ne	0x1002bf324 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x28c>
1002bf2f8: 510006a5    	sub	w5, w21, #0x1
1002bf2fc: 7100009f    	cmp	w4, #0x0
1002bf300: 1a851063    	csel	w3, w3, w5, ne
1002bf304: 1a8110a1    	csel	w1, w5, w1, ne
1002bf308: 14000011    	b	0x1002bf34c <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x2b4>
1002bf30c: 36000a44    	tbz	w4, #0x0, 0x1002bf454 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x3bc>
1002bf310: d2800000    	mov	x0, #0x0                ; =0
1002bf314: 5280000d    	mov	w13, #0x0               ; =0
1002bf318: aa1403e3    	mov	x3, x20
1002bf31c: aa1403e6    	mov	x6, x20
1002bf320: 17ffffdc    	b	0x1002bf290 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
1002bf324: f9405265    	ldr	x5, [x19, #0xa0]
1002bf328: d10004a5    	sub	x5, x5, #0x1
1002bf32c: 510006a6    	sub	w6, w21, #0x1
1002bf330: 7100009f    	cmp	w4, #0x0
1002bf334: 1a861063    	csel	w3, w3, w6, ne
1002bf338: 1a9f0610    	csinc	w16, w16, wzr, eq
1002bf33c: 9a851000    	csel	x0, x0, x5, ne
1002bf340: 1a9f15ad    	csinc	w13, w13, wzr, ne
1002bf344: 1a8110c1    	csel	w1, w6, w1, ne
1002bf348: 9a8e10ae    	csel	x14, x5, x14, ne
1002bf34c: b9404be4    	ldr	w4, [sp, #0x48]
1002bf350: b90063e4    	str	w4, [sp, #0x60]
1002bf354: b844b3e4    	ldur	w4, [sp, #0x4b]
1002bf358: b80633e4    	stur	w4, [sp, #0x63]
1002bf35c: b94043e4    	ldr	w4, [sp, #0x40]
1002bf360: b9005be4    	str	w4, [sp, #0x58]
1002bf364: b84433e4    	ldur	w4, [sp, #0x43]
1002bf368: b805b3e4    	stur	w4, [sp, #0x5b]
1002bf36c: b9403be4    	ldr	w4, [sp, #0x38]
1002bf370: b90053e4    	str	w4, [sp, #0x50]
1002bf374: b843b3e4    	ldur	w4, [sp, #0x3b]
1002bf378: b80533e4    	stur	w4, [sp, #0x53]
1002bf37c: f9012a6e    	str	x14, [x19, #0x250]
1002bf380: b9025a61    	str	w1, [x19, #0x258]
1002bf384: b9025e6f    	str	w15, [x19, #0x25c]
1002bf388: 39098270    	strb	w16, [x19, #0x260]
1002bf38c: b94063ee    	ldr	w14, [sp, #0x60]
1002bf390: b900010e    	str	w14, [x8]
1002bf394: b84633ee    	ldur	w14, [sp, #0x63]
1002bf398: b800310e    	stur	w14, [x8, #0x3]
1002bf39c: f9013660    	str	x0, [x19, #0x268]
1002bf3a0: b9027263    	str	w3, [x19, #0x270]
1002bf3a4: b9027671    	str	w17, [x19, #0x274]
1002bf3a8: 3909e26d    	strb	w13, [x19, #0x278]
1002bf3ac: b9405be8    	ldr	w8, [sp, #0x58]
1002bf3b0: b9000128    	str	w8, [x9]
1002bf3b4: b845b3e8    	ldur	w8, [sp, #0x5b]
1002bf3b8: b8003128    	stur	w8, [x9, #0x3]
1002bf3bc: 390a026b    	strb	w11, [x19, #0x280]
1002bf3c0: b94053e8    	ldr	w8, [sp, #0x50]
1002bf3c4: b84533e9    	ldur	w9, [sp, #0x53]
1002bf3c8: b8003149    	stur	w9, [x10, #0x3]
1002bf3cc: b9000148    	str	w8, [x10]
1002bf3d0: f100045f    	cmp	x2, #0x1
1002bf3d4: 540000e1    	b.ne	0x1002bf3f0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x358>
1002bf3d8: 350000d4    	cbnz	w20, 0x1002bf3f0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x358>
1002bf3dc: eb15019f    	cmp	x12, x21
1002bf3e0: 54000081    	b.ne	0x1002bf3f0 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x358>
1002bf3e4: 91004260    	add	x0, x19, #0x10
1002bf3e8: 940080f8    	bl	0x1002df7c8 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$18scroll_up_full_one17h516f76671bd0d16fE>
1002bf3ec: 14000004    	b	0x1002bf3fc <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x364>
1002bf3f0: 91004260    	add	x0, x19, #0x10
1002bf3f4: 9101a3e1    	add	x1, sp, #0x68
1002bf3f8: 9400820c    	bl	0x1002dfc28 <__ZN18alacritty_terminal4grid13Grid$LT$T$GT$9scroll_up17he403fc1484f38229E>
1002bf3fc: b940b268    	ldr	w8, [x19, #0xb0]
1002bf400: b9400fe9    	ldr	w9, [sp, #0xc]
1002bf404: 7100029f    	cmp	w20, #0x0
1002bf408: 5a881528    	csneg	w8, w9, w8, ne
1002bf40c: b9429269    	ldr	w9, [x19, #0x290]
1002bf410: 6b09011f    	cmp	w8, w9
1002bf414: 5400010c    	b.gt	0x1002bf434 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x39c>
1002bf418: 6b0902bf    	cmp	w21, w9
1002bf41c: 540000cd    	b.le	0x1002bf434 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x39c>
1002bf420: b94013ea    	ldr	w10, [sp, #0x10]
1002bf424: 4b0a0129    	sub	w9, w9, w10
1002bf428: 6b09011f    	cmp	w8, w9
1002bf42c: 1a89c108    	csel	w8, w8, w9, gt
1002bf430: b9029268    	str	w8, [x19, #0x290]
1002bf434: 52800028    	mov	w8, #0x1                ; =1
1002bf438: 39082268    	strb	w8, [x19, #0x208]
1002bf43c: a9507bfd    	ldp	x29, x30, [sp, #0x100]
1002bf440: a94f4ff4    	ldp	x20, x19, [sp, #0xf0]
1002bf444: a94e57f6    	ldp	x22, x21, [sp, #0xe0]
1002bf448: a94d6ffc    	ldp	x28, x27, [sp, #0xd0]
1002bf44c: 910443ff    	add	sp, sp, #0x110
1002bf450: d65f03c0    	ret
1002bf454: 52800010    	mov	w16, #0x0               ; =0
1002bf458: d280000e    	mov	x14, #0x0               ; =0
1002bf45c: aa1403e1    	mov	x1, x20
1002bf460: aa1403e6    	mov	x6, x20
1002bf464: 17ffff8b    	b	0x1002bf290 <__ZN18alacritty_terminal4term13Term$LT$T$GT$18scroll_up_relative17h216ddd8bd0001177E+0x1f8>
