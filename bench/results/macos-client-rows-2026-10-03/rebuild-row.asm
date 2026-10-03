
target/main-bench-20261003/loomtty.app/Contents/MacOS/loomtty:	file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004431a8 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE>:
1004431a8: 6db63bef    	stp	d15, d14, [sp, #-0xa0]!
1004431ac: 6d0133ed    	stp	d13, d12, [sp, #0x10]
1004431b0: 6d022beb    	stp	d11, d10, [sp, #0x20]
1004431b4: 6d0323e9    	stp	d9, d8, [sp, #0x30]
1004431b8: a9046ffc    	stp	x28, x27, [sp, #0x40]
1004431bc: a90567fa    	stp	x26, x25, [sp, #0x50]
1004431c0: a9065ff8    	stp	x24, x23, [sp, #0x60]
1004431c4: a90757f6    	stp	x22, x21, [sp, #0x70]
1004431c8: a9084ff4    	stp	x20, x19, [sp, #0x80]
1004431cc: a9097bfd    	stp	x29, x30, [sp, #0x90]
1004431d0: 910243fd    	add	x29, sp, #0x90
1004431d4: d10683ff    	sub	sp, sp, #0x1a0
1004431d8: f90067e4    	str	x4, [sp, #0xc8]
1004431dc: f90023e3    	str	x3, [sp, #0x40]
1004431e0: f9002be2    	str	x2, [sp, #0x50]
1004431e4: aa0003fb    	mov	x27, x0
1004431e8: f900081f    	str	xzr, [x0, #0x10]
1004431ec: f900141f    	str	xzr, [x0, #0x28]
1004431f0: f900201f    	str	xzr, [x0, #0x40]
1004431f4: b900781f    	str	wzr, [x0, #0x78]
1004431f8: 79405034    	ldrh	w20, [x1, #0x28]
1004431fc: b40072f4    	cbz	x20, 0x100444058 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xeb0>
100443200: a905fffb    	stp	x27, xzr, [sp, #0x58]
100443204: 5280000c    	mov	w12, #0x0               ; =0
100443208: 5280000d    	mov	w13, #0x0               ; =0
10044320c: 5280000e    	mov	w14, #0x0               ; =0
100443210: 5280000f    	mov	w15, #0x0               ; =0
100443214: 52800010    	mov	w16, #0x0               ; =0
100443218: f90057ff    	str	xzr, [sp, #0xa8]
10044321c: f90063ff    	str	xzr, [sp, #0xc0]
100443220: 52800011    	mov	w17, #0x0               ; =0
100443224: 52800008    	mov	w8, #0x0                ; =0
100443228: d2800019    	mov	x25, #0x0               ; =0
10044322c: d280001c    	mov	x28, #0x0               ; =0
100443230: b900a7ff    	str	wzr, [sp, #0xa4]
100443234: a940282b    	ldp	x11, x10, [x1]
100443238: a9097fea    	stp	x10, xzr, [sp, #0x90]
10044323c: a9412829    	ldp	x9, x10, [x1, #0x10]
100443240: a906a7ea    	stp	x10, x9, [sp, #0x68]
100443244: f9402be9    	ldr	x9, [sp, #0x50]
100443248: 9b147d2a    	mul	x10, x9, x20
10044324c: a9082beb    	stp	x11, x10, [sp, #0x80]
100443250: 9e230128    	ucvtf	s8, x9
100443254: 2944242a    	ldp	w10, w9, [x1, #0x20]
100443258: 29062be9    	stp	w9, w10, [sp, #0x30]
10044325c: f9001fe1    	str	x1, [sp, #0x38]
100443260: 52a00221    	mov	w1, #0x110000           ; =1114112
100443264: 1e2c100b    	fmov	s11, #0.50000000
100443268: 3d802fe0    	str	q0, [sp, #0xb0]
10044326c: f9003ff4    	str	x20, [sp, #0x78]
100443270: a948abe9    	ldp	x9, x10, [sp, #0x88]
100443274: 8b090329    	add	x9, x25, x9
100443278: eb0a013f    	cmp	x9, x10
10044327c: 54006b22    	b.hs	0x100443fe0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xe38>
100443280: f94043ea    	ldr	x10, [sp, #0x80]
100443284: 8b091140    	add	x0, x10, x9, lsl #4
100443288: f8404009    	ldur	x9, [x0, #0x4]
10044328c: 79401817    	ldrh	w23, [x0, #0xc]
100443290: 12001d3b    	and	w27, w9, #0xff
100443294: 6b1b023f    	cmp	w17, w27
100443298: 1a9f17ea    	cset	w10, eq
10044329c: 0a0a0108    	and	w8, w8, w10
1004432a0: 53083d2a    	ubfx	w10, w9, #8, #8
1004432a4: 53105d2b    	ubfx	w11, w9, #16, #8
1004432a8: 7100051f    	cmp	w8, #0x1
1004432ac: b940c7e8    	ldr	w8, [sp, #0xc4]
1004432b0: 7a4a0100    	ccmp	w8, w10, #0x0, eq
1004432b4: d3609d35    	ubfx	x21, x9, #32, #8
1004432b8: d368bd3a    	ubfx	x26, x9, #40, #8
1004432bc: b940c3e8    	ldr	w8, [sp, #0xc0]
1004432c0: 7a4b0100    	ccmp	w8, w11, #0x0, eq
1004432c4: d370dd38    	ubfx	x24, x9, #48, #8
1004432c8: d378fd36    	lsr	x22, x9, #56
1004432cc: 54000121    	b.ne	0x1004432f0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x148>
1004432d0: f94057e8    	ldr	x8, [sp, #0xa8]
1004432d4: 6b49611f    	cmp	w8, w9, lsr #24
1004432d8: 7a550200    	ccmp	w16, w21, #0x0, eq
1004432dc: 7a5a01e0    	ccmp	w15, w26, #0x0, eq
1004432e0: 7a5801c0    	ccmp	w14, w24, #0x0, eq
1004432e4: 7a5601a0    	ccmp	w13, w22, #0x0, eq
1004432e8: 7a570180    	ccmp	w12, w23, #0x0, eq
1004432ec: 54000380    	b.eq	0x10044335c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x1b4>
1004432f0: 53083d2a    	ubfx	w10, w9, #8, #8
1004432f4: 53105d28    	ubfx	w8, w9, #16, #8
1004432f8: 29182be8    	stp	w8, w10, [sp, #0xc0]
1004432fc: 53187d28    	lsr	w8, w9, #24
100443300: f90057e8    	str	x8, [sp, #0xa8]
100443304: d10333a8    	sub	x8, x29, #0xcc
100443308: f94037e1    	ldr	x1, [sp, #0x68]
10044330c: 94001959    	bl	0x100449870 <__ZN11loom_render8terminal4cell9CellProps21from_packed_cell_fast17hcaadd4727e3edd55E>
100443310: b85343a1    	ldur	w1, [x29, #-0xcc]
100443314: d10333a9    	sub	x9, x29, #0xcc
100443318: f8424128    	ldur	x8, [x9, #0x24]
10044331c: f9007be8    	str	x8, [sp, #0xf0]
100443320: 3cc14120    	ldur	q0, [x9, #0x14]
100443324: 3cc04121    	ldur	q1, [x9, #0x4]
100443328: ad0683e1    	stp	q1, q0, [sp, #0xd0]
10044332c: aa1703ec    	mov	x12, x23
100443330: aa1603ed    	mov	x13, x22
100443334: aa1803ee    	mov	x14, x24
100443338: aa1a03ef    	mov	x15, x26
10044333c: ad0803e1    	stp	q1, q0, [sp, #0x100]
100443340: f90093e8    	str	x8, [sp, #0x120]
100443344: aa1503f0    	mov	x16, x21
100443348: aa1b03f1    	mov	x17, x27
10044334c: aa0103e0    	mov	x0, x1
100443350: 7144403f    	cmp	w1, #0x110, lsl #12     ; =0x110000
100443354: 54000080    	b.eq	0x100443364 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x1bc>
100443358: 14000018    	b	0x1004433b8 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x210>
10044335c: 7144403f    	cmp	w1, #0x110, lsl #12     ; =0x110000
100443360: 54000061    	b.ne	0x10044336c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x1c4>
100443364: 52a00221    	mov	w1, #0x110000           ; =1114112
100443368: 140000da    	b	0x1004436d0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x528>
10044336c: ad4687e0    	ldp	q0, q1, [sp, #0xd0]
100443370: ad0807e0    	stp	q0, q1, [sp, #0x100]
100443374: f9407be8    	ldr	x8, [sp, #0xf0]
100443378: f90093e8    	str	x8, [sp, #0x120]
10044337c: aa0c03f5    	mov	x21, x12
100443380: aa0d03f6    	mov	x22, x13
100443384: aa0e03f7    	mov	x23, x14
100443388: aa0f03f8    	mov	x24, x15
10044338c: aa1003fa    	mov	x26, x16
100443390: aa1103f4    	mov	x20, x17
100443394: aa0103fb    	mov	x27, x1
100443398: 9408d3cd    	bl	0x1006782cc <__ZN13loom_protocol7message10PackedCell2ch17hc34a550b7b1c3fcdE>
10044339c: aa1b03e1    	mov	x1, x27
1004433a0: aa1403f1    	mov	x17, x20
1004433a4: aa1a03f0    	mov	x16, x26
1004433a8: aa1803ef    	mov	x15, x24
1004433ac: aa1703ee    	mov	x14, x23
1004433b0: aa1603ed    	mov	x13, x22
1004433b4: aa1503ec    	mov	x12, x21
1004433b8: b81343a0    	stur	w0, [x29, #-0xcc]
1004433bc: f94093e8    	ldr	x8, [sp, #0x120]
1004433c0: d10333a9    	sub	x9, x29, #0xcc
1004433c4: f8024128    	stur	x8, [x9, #0x24]
1004433c8: ad4803e1    	ldp	q1, q0, [sp, #0x100]
1004433cc: 3c814120    	stur	q0, [x9, #0x14]
1004433d0: 3c804121    	stur	q1, [x9, #0x4]
1004433d4: bc5483a0    	ldur	s0, [x29, #-0xb8]
1004433d8: f9403be8    	ldr	x8, [sp, #0x70]
1004433dc: bd400101    	ldr	s1, [x8]
1004433e0: 1e212000    	fcmp	s0, s1
1004433e4: 540007e1    	b.ne	0x1004434e0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x338>
1004433e8: bc54c3a1    	ldur	s1, [x29, #-0xb4]
1004433ec: bd400502    	ldr	s2, [x8, #0x4]
1004433f0: 1e222020    	fcmp	s1, s2
1004433f4: 54000761    	b.ne	0x1004434e0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x338>
1004433f8: bc5503a1    	ldur	s1, [x29, #-0xb0]
1004433fc: bd400902    	ldr	s2, [x8, #0x8]
100443400: 1e222020    	fcmp	s1, s2
100443404: 540006e1    	b.ne	0x1004434e0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x338>
100443408: bc5543a1    	ldur	s1, [x29, #-0xac]
10044340c: bd400d02    	ldr	s2, [x8, #0xc]
100443410: 1e222020    	fcmp	s1, s2
100443414: 54000661    	b.ne	0x1004434e0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x338>
100443418: b940a7e9    	ldr	w9, [sp, #0xa4]
10044341c: 36001109    	tbz	w9, #0x0, 0x10044363c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x494>
100443420: 2d42310d    	ldp	s13, s12, [x8, #0x10]
100443424: f9402ff6    	ldr	x22, [sp, #0x58]
100443428: f94022d5    	ldr	x21, [x22, #0x40]
10044342c: f9401ac8    	ldr	x8, [x22, #0x30]
100443430: eb0802bf    	cmp	x21, x8
100443434: 540002a1    	b.ne	0x100443488 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x2e0>
100443438: aa0003f3    	mov	x19, x0
10044343c: 9100c2c0    	add	x0, x22, #0x30
100443440: aa0c03f7    	mov	x23, x12
100443444: f90027fc    	str	x28, [sp, #0x48]
100443448: aa0d03fc    	mov	x28, x13
10044344c: aa0e03fa    	mov	x26, x14
100443450: aa0f03f8    	mov	x24, x15
100443454: b9002bf0    	str	w16, [sp, #0x28]
100443458: aa1103f4    	mov	x20, x17
10044345c: aa0103fb    	mov	x27, x1
100443460: 94101344    	bl	0x100848170 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hca29980afef496daE>
100443464: aa1303e0    	mov	x0, x19
100443468: aa1b03e1    	mov	x1, x27
10044346c: aa1403f1    	mov	x17, x20
100443470: b9402bf0    	ldr	w16, [sp, #0x28]
100443474: aa1803ef    	mov	x15, x24
100443478: aa1a03ee    	mov	x14, x26
10044347c: aa1c03ed    	mov	x13, x28
100443480: f94027fc    	ldr	x28, [sp, #0x48]
100443484: aa1703ec    	mov	x12, x23
100443488: b900a7ff    	str	wzr, [sp, #0xa4]
10044348c: f94033e8    	ldr	x8, [sp, #0x60]
100443490: 9e230100    	ucvtf	s0, x8
100443494: 1e2009a0    	fmul	s0, s13, s0
100443498: cb080328    	sub	x8, x25, x8
10044349c: 9e230101    	ucvtf	s1, x8
1004434a0: 1e2109a1    	fmul	s1, s13, s1
1004434a4: f9401ec8    	ldr	x8, [x22, #0x38]
1004434a8: 1e280982    	fmul	s2, s12, s8
1004434ac: 8b151508    	add	x8, x8, x21, lsl #5
1004434b0: 2d000900    	stp	s0, s2, [x8]
1004434b4: 2d013101    	stp	s1, s12, [x8, #0x8]
1004434b8: bd001109    	str	s9, [x8, #0x10]
1004434bc: 3dc02fe0    	ldr	q0, [sp, #0xb0]
1004434c0: fc014100    	stur	d0, [x8, #0x14]
1004434c4: bd001d0a    	str	s10, [x8, #0x1c]
1004434c8: 910006a8    	add	x8, x21, #0x1
1004434cc: f90022c8    	str	x8, [x22, #0x40]
1004434d0: f9404fe9    	ldr	x9, [sp, #0x98]
1004434d4: 3855c3a8    	ldurb	w8, [x29, #-0xa4]
1004434d8: 34000ba8    	cbz	w8, 0x10044364c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4a4>
1004434dc: 14000061    	b	0x100443660 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4b8>
1004434e0: b940a7e9    	ldr	w9, [sp, #0xa4]
1004434e4: 36000269    	tbz	w9, #0x0, 0x100443530 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x388>
1004434e8: 1e202120    	fcmp	s9, s0
1004434ec: f9404fe9    	ldr	x9, [sp, #0x98]
1004434f0: 54000381    	b.ne	0x100443560 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x3b8>
1004434f4: bc54c3a0    	ldur	s0, [x29, #-0xb4]
1004434f8: 3dc02fe1    	ldr	q1, [sp, #0xb0]
1004434fc: 1e202020    	fcmp	s1, s0
100443500: 54000301    	b.ne	0x100443560 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x3b8>
100443504: bc5503a0    	ldur	s0, [x29, #-0xb0]
100443508: 1e202180    	fcmp	s12, s0
10044350c: 540002a1    	b.ne	0x100443560 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x3b8>
100443510: bc5543a0    	ldur	s0, [x29, #-0xac]
100443514: 1e202140    	fcmp	s10, s0
100443518: 54000241    	b.ne	0x100443560 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x3b8>
10044351c: 52800028    	mov	w8, #0x1                ; =1
100443520: b900a7e8    	str	w8, [sp, #0xa4]
100443524: 3855c3a8    	ldurb	w8, [x29, #-0xa4]
100443528: 34000928    	cbz	w8, 0x10044364c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4a4>
10044352c: 1400004d    	b	0x100443660 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4b8>
100443530: d10333a8    	sub	x8, x29, #0xcc
100443534: fd400d01    	ldr	d1, [x8, #0x18]
100443538: 3d802fe1    	str	q1, [sp, #0xb0]
10044353c: 52800028    	mov	w8, #0x1                ; =1
100443540: b900a7e8    	str	w8, [sp, #0xa4]
100443544: bc5543aa    	ldur	s10, [x29, #-0xac]
100443548: 1e604009    	fmov	d9, d0
10044354c: f90033f9    	str	x25, [sp, #0x60]
100443550: f9404fe9    	ldr	x9, [sp, #0x98]
100443554: 3855c3a8    	ldurb	w8, [x29, #-0xa4]
100443558: 340007a8    	cbz	w8, 0x10044364c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4a4>
10044355c: 14000041    	b	0x100443660 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4b8>
100443560: 2d42310d    	ldp	s13, s12, [x8, #0x10]
100443564: f9402ff6    	ldr	x22, [sp, #0x58]
100443568: f94022d5    	ldr	x21, [x22, #0x40]
10044356c: f9401ac8    	ldr	x8, [x22, #0x30]
100443570: eb0802bf    	cmp	x21, x8
100443574: 540002c1    	b.ne	0x1004435cc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x424>
100443578: b900a7e0    	str	w0, [sp, #0xa4]
10044357c: 9100c2c0    	add	x0, x22, #0x30
100443580: aa0c03f7    	mov	x23, x12
100443584: f90027fc    	str	x28, [sp, #0x48]
100443588: aa0d03fc    	mov	x28, x13
10044358c: aa0e03fa    	mov	x26, x14
100443590: aa0f03f8    	mov	x24, x15
100443594: aa1003fb    	mov	x27, x16
100443598: aa1103f4    	mov	x20, x17
10044359c: aa0103f3    	mov	x19, x1
1004435a0: 941012f4    	bl	0x100848170 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hca29980afef496daE>
1004435a4: b940a7e0    	ldr	w0, [sp, #0xa4]
1004435a8: aa1303e1    	mov	x1, x19
1004435ac: aa1403f1    	mov	x17, x20
1004435b0: aa1b03f0    	mov	x16, x27
1004435b4: aa1803ef    	mov	x15, x24
1004435b8: aa1a03ee    	mov	x14, x26
1004435bc: aa1c03ed    	mov	x13, x28
1004435c0: f94027fc    	ldr	x28, [sp, #0x48]
1004435c4: aa1703ec    	mov	x12, x23
1004435c8: f9404fe9    	ldr	x9, [sp, #0x98]
1004435cc: f94033e8    	ldr	x8, [sp, #0x60]
1004435d0: 9e230100    	ucvtf	s0, x8
1004435d4: 1e2009a0    	fmul	s0, s13, s0
1004435d8: cb080328    	sub	x8, x25, x8
1004435dc: 9e230101    	ucvtf	s1, x8
1004435e0: 1e2109a1    	fmul	s1, s13, s1
1004435e4: 1e280982    	fmul	s2, s12, s8
1004435e8: f9401ec8    	ldr	x8, [x22, #0x38]
1004435ec: 8b151508    	add	x8, x8, x21, lsl #5
1004435f0: 2d000900    	stp	s0, s2, [x8]
1004435f4: 2d013101    	stp	s1, s12, [x8, #0x8]
1004435f8: bd001109    	str	s9, [x8, #0x10]
1004435fc: 3dc02fe0    	ldr	q0, [sp, #0xb0]
100443600: fc014100    	stur	d0, [x8, #0x14]
100443604: bd001d0a    	str	s10, [x8, #0x1c]
100443608: 910006a8    	add	x8, x21, #0x1
10044360c: f90022c8    	str	x8, [x22, #0x40]
100443610: d10333a8    	sub	x8, x29, #0xcc
100443614: fd400d00    	ldr	d0, [x8, #0x18]
100443618: 3d802fe0    	str	q0, [sp, #0xb0]
10044361c: 52800028    	mov	w8, #0x1                ; =1
100443620: b900a7e8    	str	w8, [sp, #0xa4]
100443624: bc5483a9    	ldur	s9, [x29, #-0xb8]
100443628: bc5543aa    	ldur	s10, [x29, #-0xac]
10044362c: f90033f9    	str	x25, [sp, #0x60]
100443630: 3855c3a8    	ldurb	w8, [x29, #-0xa4]
100443634: 340000c8    	cbz	w8, 0x10044364c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4a4>
100443638: 1400000a    	b	0x100443660 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4b8>
10044363c: b900a7ff    	str	wzr, [sp, #0xa4]
100443640: f9404fe9    	ldr	x9, [sp, #0x98]
100443644: 3855c3a8    	ldurb	w8, [x29, #-0xa4]
100443648: 350000c8    	cbnz	w8, 0x100443660 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4b8>
10044364c: 3855a3a8    	ldurb	w8, [x29, #-0xa6]
100443650: 37000088    	tbnz	w8, #0x0, 0x100443660 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x4b8>
100443654: 385593a8    	ldurb	w8, [x29, #-0xa7]
100443658: 370003c8    	tbnz	w8, #0x0, 0x1004436d0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x528>
10044365c: 1400001b    	b	0x1004436c8 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x520>
100443660: d10333a2    	sub	x2, x29, #0xcc
100443664: f9402fe8    	ldr	x8, [sp, #0x58]
100443668: 9100c104    	add	x4, x8, #0x30
10044366c: aa0003f3    	mov	x19, x0
100443670: f9402be0    	ldr	x0, [sp, #0x50]
100443674: aa0103fb    	mov	x27, x1
100443678: aa1903e1    	mov	x1, x25
10044367c: f9403be3    	ldr	x3, [sp, #0x70]
100443680: aa0c03f5    	mov	x21, x12
100443684: aa0d03f6    	mov	x22, x13
100443688: aa0e03f7    	mov	x23, x14
10044368c: aa0f03f8    	mov	x24, x15
100443690: aa1003fa    	mov	x26, x16
100443694: aa1103f4    	mov	x20, x17
100443698: 94003233    	bl	0x10044ff64 <__ZN11loom_render8terminal10decoration23render_cell_decorations17he72d3a0e7f4090a3E>
10044369c: aa1303e0    	mov	x0, x19
1004436a0: aa1b03e1    	mov	x1, x27
1004436a4: aa1403f1    	mov	x17, x20
1004436a8: aa1a03f0    	mov	x16, x26
1004436ac: aa1803ef    	mov	x15, x24
1004436b0: aa1703ee    	mov	x14, x23
1004436b4: aa1603ed    	mov	x13, x22
1004436b8: aa1503ec    	mov	x12, x21
1004436bc: f9404fe9    	ldr	x9, [sp, #0x98]
1004436c0: 385593a8    	ldurb	w8, [x29, #-0xa7]
1004436c4: 37000068    	tbnz	w8, #0x0, 0x1004436d0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x528>
1004436c8: 721a781f    	tst	w0, #0xffffffdf
1004436cc: 54000121    	b.ne	0x1004436f0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x548>
1004436d0: f9403ff4    	ldr	x20, [sp, #0x78]
1004436d4: 91000739    	add	x25, x25, #0x1
1004436d8: 3dc02fe0    	ldr	q0, [sp, #0xb0]
1004436dc: 5e0c040c    	mov	s12, v0[1]
1004436e0: 52800028    	mov	w8, #0x1                ; =1
1004436e4: eb14033f    	cmp	x25, x20
1004436e8: 54ffdc41    	b.ne	0x100443270 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xc8>
1004436ec: 1400023d    	b	0x100443fe0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xe38>
1004436f0: 7100801f    	cmp	w0, #0x20
1004436f4: 54fffee3    	b.lo	0x1004436d0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x528>
1004436f8: 5101fc08    	sub	w8, w0, #0x7f
1004436fc: 7100851f    	cmp	w8, #0x21
100443700: 54fffe83    	b.lo	0x1004436d0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x528>
100443704: 12849fe8    	mov	w8, #-0x2500            ; =-9472
100443708: 0b080008    	add	w8, w0, w8
10044370c: 7102811f    	cmp	w8, #0xa0
100443710: f90027fc    	str	x28, [sp, #0x48]
100443714: 54000902    	b.hs	0x100443834 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x68c>
100443718: b940a7e8    	ldr	w8, [sp, #0xa4]
10044371c: b9002fee    	str	w14, [sp, #0x2c]
100443720: 34000568    	cbz	w8, 0x1004437cc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x624>
100443724: f94033e8    	ldr	x8, [sp, #0x60]
100443728: eb080336    	subs	x22, x25, x8
10044372c: 54000509    	b.ls	0x1004437cc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x624>
100443730: f9403be8    	ldr	x8, [sp, #0x70]
100443734: 2d42310d    	ldp	s13, s12, [x8, #0x10]
100443738: f9402ff7    	ldr	x23, [sp, #0x58]
10044373c: f94022f5    	ldr	x21, [x23, #0x40]
100443740: f9401ae8    	ldr	x8, [x23, #0x30]
100443744: eb0802bf    	cmp	x21, x8
100443748: 54000221    	b.ne	0x10044378c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x5e4>
10044374c: aa0003f3    	mov	x19, x0
100443750: 9100c2e0    	add	x0, x23, #0x30
100443754: aa0c03fc    	mov	x28, x12
100443758: b900a7ed    	str	w13, [sp, #0xa4]
10044375c: aa0f03fa    	mov	x26, x15
100443760: aa1003f8    	mov	x24, x16
100443764: aa1103fb    	mov	x27, x17
100443768: aa0103f4    	mov	x20, x1
10044376c: 94101281    	bl	0x100848170 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hca29980afef496daE>
100443770: aa1303e0    	mov	x0, x19
100443774: aa1403e1    	mov	x1, x20
100443778: aa1b03f1    	mov	x17, x27
10044377c: aa1803f0    	mov	x16, x24
100443780: aa1a03ef    	mov	x15, x26
100443784: b940a7ed    	ldr	w13, [sp, #0xa4]
100443788: aa1c03ec    	mov	x12, x28
10044378c: f94033e8    	ldr	x8, [sp, #0x60]
100443790: 9e230100    	ucvtf	s0, x8
100443794: 1e2009a0    	fmul	s0, s13, s0
100443798: 9e2302c1    	ucvtf	s1, x22
10044379c: 1e2109a1    	fmul	s1, s13, s1
1004437a0: f9401ee8    	ldr	x8, [x23, #0x38]
1004437a4: 1e280982    	fmul	s2, s12, s8
1004437a8: 8b151508    	add	x8, x8, x21, lsl #5
1004437ac: 2d000900    	stp	s0, s2, [x8]
1004437b0: 2d013101    	stp	s1, s12, [x8, #0x8]
1004437b4: bd001109    	str	s9, [x8, #0x10]
1004437b8: 3dc02fe0    	ldr	q0, [sp, #0xb0]
1004437bc: fc014100    	stur	d0, [x8, #0x14]
1004437c0: bd001d0a    	str	s10, [x8, #0x1c]
1004437c4: 910006a8    	add	x8, x21, #0x1
1004437c8: f90022e8    	str	x8, [x23, #0x40]
1004437cc: bc5483a0    	ldur	s0, [x29, #-0xb8]
1004437d0: f9403be8    	ldr	x8, [sp, #0x70]
1004437d4: bd400101    	ldr	s1, [x8]
1004437d8: 1e212000    	fcmp	s0, s1
1004437dc: 54000301    	b.ne	0x10044383c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x694>
1004437e0: bc54c3a0    	ldur	s0, [x29, #-0xb4]
1004437e4: bd400501    	ldr	s1, [x8, #0x4]
1004437e8: 1e212000    	fcmp	s0, s1
1004437ec: 54000281    	b.ne	0x10044383c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x694>
1004437f0: bc5503a0    	ldur	s0, [x29, #-0xb0]
1004437f4: bd400901    	ldr	s1, [x8, #0x8]
1004437f8: 1e212000    	fcmp	s0, s1
1004437fc: 54000201    	b.ne	0x10044383c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x694>
100443800: bc5543a0    	ldur	s0, [x29, #-0xac]
100443804: bd400d01    	ldr	s1, [x8, #0xc]
100443808: 1e212000    	fcmp	s0, s1
10044380c: 54000181    	b.ne	0x10044383c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x694>
100443810: aa0103f4    	mov	x20, x1
100443814: aa1103fb    	mov	x27, x17
100443818: aa1003f8    	mov	x24, x16
10044381c: aa0f03fa    	mov	x26, x15
100443820: aa0d03fc    	mov	x28, x13
100443824: aa0c03f7    	mov	x23, x12
100443828: 9e230320    	ucvtf	s0, x25
10044382c: f9402ff6    	ldr	x22, [sp, #0x58]
100443830: 14000027    	b	0x1004438cc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x724>
100443834: aa0003f6    	mov	x22, x0
100443838: 1400003a    	b	0x100443920 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x778>
10044383c: aa0103f4    	mov	x20, x1
100443840: aa1103fb    	mov	x27, x17
100443844: aa1003f8    	mov	x24, x16
100443848: aa0f03fa    	mov	x26, x15
10044384c: aa0d03fc    	mov	x28, x13
100443850: aa0c03f7    	mov	x23, x12
100443854: aa0803e9    	mov	x9, x8
100443858: 385583a8    	ldurb	w8, [x29, #-0xa8]
10044385c: 2d42312d    	ldp	s13, s12, [x9, #0x10]
100443860: 1e2d29a0    	fadd	s0, s13, s13
100443864: 7100011f    	cmp	w8, #0x0
100443868: 1e2d1c0e    	fcsel	s14, s0, s13, ne
10044386c: d10333a8    	sub	x8, x29, #0xcc
100443870: 3cc14100    	ldur	q0, [x8, #0x14]
100443874: 3d8043e0    	str	q0, [sp, #0x100]
100443878: f9402ff6    	ldr	x22, [sp, #0x58]
10044387c: f94022d5    	ldr	x21, [x22, #0x40]
100443880: f9401ac8    	ldr	x8, [x22, #0x30]
100443884: eb0802bf    	cmp	x21, x8
100443888: 540000a1    	b.ne	0x10044389c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x6f4>
10044388c: aa0003f3    	mov	x19, x0
100443890: 9100c2c0    	add	x0, x22, #0x30
100443894: 94101237    	bl	0x100848170 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hca29980afef496daE>
100443898: aa1303e0    	mov	x0, x19
10044389c: 9e230320    	ucvtf	s0, x25
1004438a0: 1e2009a1    	fmul	s1, s13, s0
1004438a4: 1e280982    	fmul	s2, s12, s8
1004438a8: f9401ec8    	ldr	x8, [x22, #0x38]
1004438ac: 8b151508    	add	x8, x8, x21, lsl #5
1004438b0: 2d000901    	stp	s1, s2, [x8]
1004438b4: 2d01310e    	stp	s14, s12, [x8, #0x8]
1004438b8: 3dc043e1    	ldr	q1, [sp, #0x100]
1004438bc: 3d800501    	str	q1, [x8, #0x10]
1004438c0: 910006a8    	add	x8, x21, #0x1
1004438c4: f90022c8    	str	x8, [x22, #0x40]
1004438c8: f9403be8    	ldr	x8, [sp, #0x70]
1004438cc: 2d420d02    	ldp	s2, s3, [x8, #0x10]
1004438d0: 1e220800    	fmul	s0, s0, s2
1004438d4: 1e280861    	fmul	s1, s3, s8
1004438d8: d10333a8    	sub	x8, x29, #0xcc
1004438dc: 3cc04104    	ldur	q4, [x8, #0x4]
1004438e0: 3d8043e4    	str	q4, [sp, #0x100]
1004438e4: 910403e1    	add	x1, sp, #0x100
1004438e8: 9100c2c2    	add	x2, x22, #0x30
1004438ec: aa0003f6    	mov	x22, x0
1004438f0: 940035e9    	bl	0x100451094 <__ZN11loom_render8terminal11box_drawing4emit17h604961f497df4dacE>
1004438f4: b900a7ff    	str	wzr, [sp, #0xa4]
1004438f8: f9404fe9    	ldr	x9, [sp, #0x98]
1004438fc: aa1703ec    	mov	x12, x23
100443900: aa1c03ed    	mov	x13, x28
100443904: b9402fee    	ldr	w14, [sp, #0x2c]
100443908: aa1a03ef    	mov	x15, x26
10044390c: aa1803f0    	mov	x16, x24
100443910: aa1b03f1    	mov	x17, x27
100443914: aa1403e1    	mov	x1, x20
100443918: f94027fc    	ldr	x28, [sp, #0x48]
10044391c: 3707eda0    	tbnz	w0, #0x0, 0x1004436d0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x528>
100443920: f94023eb    	ldr	x11, [sp, #0x40]
100443924: b4000a8b    	cbz	x11, 0x100443a74 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x8cc>
100443928: f9400968    	ldr	x8, [x11, #0x10]
10044392c: eb08033f    	cmp	x25, x8
100443930: 54000082    	b.hs	0x100443940 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x798>
100443934: f9400568    	ldr	x8, [x11, #0x8]
100443938: 38796908    	ldrb	w8, [x8, x25]
10044393c: 3707eca8    	tbnz	w8, #0x0, 0x1004436d0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x528>
100443940: f9402168    	ldr	x8, [x11, #0x40]
100443944: eb08013f    	cmp	x9, x8
100443948: 54000602    	b.hs	0x100443a08 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x860>
10044394c: aa0903ea    	mov	x10, x9
100443950: f9401d69    	ldr	x9, [x11, #0x38]
100443954: 8b0a1529    	add	x9, x9, x10, lsl #5
100443958: 91004129    	add	x9, x9, #0x10
10044395c: f85f812a    	ldur	x10, [x9, #-0x8]
100443960: eb19015f    	cmp	x10, x25
100443964: 54000122    	b.hs	0x100443988 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x7e0>
100443968: f9404fea    	ldr	x10, [sp, #0x98]
10044396c: 9100054a    	add	x10, x10, #0x1
100443970: 91008129    	add	x9, x9, #0x20
100443974: f9004fea    	str	x10, [sp, #0x98]
100443978: eb0a011f    	cmp	x8, x10
10044397c: 54ffff01    	b.ne	0x10044395c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x7b4>
100443980: aa0803e9    	mov	x9, x8
100443984: 14000021    	b	0x100443a08 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x860>
100443988: 540003e1    	b.ne	0x100443a04 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x85c>
10044398c: 2904c3e1    	stp	w1, w16, [sp, #0x24]
100443990: aa1103fb    	mov	x27, x17
100443994: aa0f03fa    	mov	x26, x15
100443998: b9002fee    	str	w14, [sp, #0x2c]
10044399c: aa0d03f7    	mov	x23, x13
1004439a0: aa0c03fc    	mov	x28, x12
1004439a4: b9400121    	ldr	w1, [x9]
1004439a8: 297e5122    	ldp	w2, w20, [x9, #-0x10]
1004439ac: f9400535    	ldr	x21, [x9, #0x8]
1004439b0: 385583a5    	ldurb	w5, [x29, #-0xa8]
1004439b4: 3855b3a4    	ldurb	w4, [x29, #-0xa5]
1004439b8: 910403e8    	add	x8, sp, #0x100
1004439bc: f94067e0    	ldr	x0, [sp, #0xc8]
1004439c0: aa0203f3    	mov	x19, x2
1004439c4: aa1403e3    	mov	x3, x20
1004439c8: aa0503f8    	mov	x24, x5
1004439cc: 940046cb    	bl	0x1004554f8 <__ZN11loom_render11glyph_cache10GlyphCache15ensure_glyph_id17ha20849e4f677fe8bE>
1004439d0: 394473e8    	ldrb	w8, [sp, #0x11c]
1004439d4: 7100091f    	cmp	w8, #0x2
1004439d8: 54000d81    	b.ne	0x100443b88 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x9e0>
1004439dc: f9404fe9    	ldr	x9, [sp, #0x98]
1004439e0: 91000529    	add	x9, x9, #0x1
1004439e4: aa1c03ec    	mov	x12, x28
1004439e8: aa1703ed    	mov	x13, x23
1004439ec: 29453bf0    	ldp	w16, w14, [sp, #0x28]
1004439f0: aa1a03ef    	mov	x15, x26
1004439f4: aa1b03f1    	mov	x17, x27
1004439f8: b94027e1    	ldr	w1, [sp, #0x24]
1004439fc: a94473eb    	ldp	x11, x28, [sp, #0x40]
100443a00: 14000002    	b	0x100443a08 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x860>
100443a04: f9404fe9    	ldr	x9, [sp, #0x98]
100443a08: f9402d68    	ldr	x8, [x11, #0x58]
100443a0c: eb08039f    	cmp	x28, x8
100443a10: f9004fe9    	str	x9, [sp, #0x98]
100443a14: 54000302    	b.hs	0x100443a74 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x8cc>
100443a18: f9402969    	ldr	x9, [x11, #0x50]
100443a1c: 5280030a    	mov	w10, #0x18              ; =24
100443a20: 9b0a2789    	madd	x9, x28, x10, x9
100443a24: 91000780    	add	x0, x28, #0x1
100443a28: 91004129    	add	x9, x9, #0x10
100443a2c: cb0803ea    	neg	x10, x8
100443a30: f85f812b    	ldur	x11, [x9, #-0x8]
100443a34: eb19017f    	cmp	x11, x25
100443a38: 540005a2    	b.hs	0x100443aec <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x944>
100443a3c: 91000400    	add	x0, x0, #0x1
100443a40: 91006129    	add	x9, x9, #0x18
100443a44: 8b00014b    	add	x11, x10, x0
100443a48: f100057f    	cmp	x11, #0x1
100443a4c: 54ffff21    	b.ne	0x100443a30 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x888>
100443a50: aa0103fc    	mov	x28, x1
100443a54: aa1103fb    	mov	x27, x17
100443a58: aa1003f8    	mov	x24, x16
100443a5c: aa0f03fa    	mov	x26, x15
100443a60: aa0e03f7    	mov	x23, x14
100443a64: aa0d03f6    	mov	x22, x13
100443a68: aa0c03f5    	mov	x21, x12
100443a6c: f90027e8    	str	x8, [sp, #0x48]
100443a70: 14000008    	b	0x100443a90 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x8e8>
100443a74: aa0103fc    	mov	x28, x1
100443a78: aa1103fb    	mov	x27, x17
100443a7c: aa1003f8    	mov	x24, x16
100443a80: aa0f03fa    	mov	x26, x15
100443a84: aa0e03f7    	mov	x23, x14
100443a88: aa0d03f6    	mov	x22, x13
100443a8c: aa0c03f5    	mov	x21, x12
100443a90: f9403be3    	ldr	x3, [sp, #0x70]
100443a94: d10333a2    	sub	x2, x29, #0xcc
100443a98: a94517e1    	ldp	x1, x5, [sp, #0x50]
100443a9c: 910060a6    	add	x6, x5, #0x18
100443aa0: aa1903e0    	mov	x0, x25
100443aa4: f94067e4    	ldr	x4, [sp, #0xc8]
100443aa8: 940038bf    	bl	0x100451da4 <__ZN11loom_render8terminal5glyph10emit_glyph17h6eab977355e0497fE>
100443aac: f9403ff4    	ldr	x20, [sp, #0x78]
100443ab0: aa1503ec    	mov	x12, x21
100443ab4: aa1603ed    	mov	x13, x22
100443ab8: aa1703ee    	mov	x14, x23
100443abc: aa1a03ef    	mov	x15, x26
100443ac0: aa1803f0    	mov	x16, x24
100443ac4: aa1b03f1    	mov	x17, x27
100443ac8: aa1c03e1    	mov	x1, x28
100443acc: f94027fc    	ldr	x28, [sp, #0x48]
100443ad0: 91000739    	add	x25, x25, #0x1
100443ad4: 3dc02fe0    	ldr	q0, [sp, #0xb0]
100443ad8: 5e0c040c    	mov	s12, v0[1]
100443adc: 52800028    	mov	w8, #0x1                ; =1
100443ae0: eb14033f    	cmp	x25, x20
100443ae4: 54ffbc61    	b.ne	0x100443270 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xc8>
100443ae8: 1400013e    	b	0x100443fe0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xe38>
100443aec: f90027e0    	str	x0, [sp, #0x48]
100443af0: eb19017f    	cmp	x11, x25
100443af4: 54000341    	b.ne	0x100443b5c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x9b4>
100443af8: aa0103f8    	mov	x24, x1
100443afc: 29043ff1    	stp	w17, w15, [sp, #0x20]
100443b00: 29053bf0    	stp	w16, w14, [sp, #0x28]
100443b04: aa0d03f7    	mov	x23, x13
100443b08: aa0c03fc    	mov	x28, x12
100443b0c: b9400121    	ldr	w1, [x9]
100443b10: 297e0d22    	ldp	w2, w3, [x9, #-0x10]
100443b14: 39401125    	ldrb	w5, [x9, #0x4]
100443b18: 3855b3a4    	ldurb	w4, [x29, #-0xa5]
100443b1c: 910403e8    	add	x8, sp, #0x100
100443b20: f94067e0    	ldr	x0, [sp, #0xc8]
100443b24: aa0203fa    	mov	x26, x2
100443b28: aa0303f3    	mov	x19, x3
100443b2c: aa0503fb    	mov	x27, x5
100443b30: 94004672    	bl	0x1004554f8 <__ZN11loom_render11glyph_cache10GlyphCache15ensure_glyph_id17ha20849e4f677fe8bE>
100443b34: 394473f5    	ldrb	w21, [sp, #0x11c]
100443b38: 71000abf    	cmp	w21, #0x2
100443b3c: 540006a1    	b.ne	0x100443c10 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xa68>
100443b40: f9403ff4    	ldr	x20, [sp, #0x78]
100443b44: aa1c03ec    	mov	x12, x28
100443b48: aa1703ed    	mov	x13, x23
100443b4c: 29453bf0    	ldp	w16, w14, [sp, #0x28]
100443b50: 29443ff1    	ldp	w17, w15, [sp, #0x20]
100443b54: aa1803e1    	mov	x1, x24
100443b58: 17ffffdd    	b	0x100443acc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x924>
100443b5c: aa0103fc    	mov	x28, x1
100443b60: aa1103fb    	mov	x27, x17
100443b64: aa1003f8    	mov	x24, x16
100443b68: aa0f03fa    	mov	x26, x15
100443b6c: aa0e03f7    	mov	x23, x14
100443b70: aa0d03f6    	mov	x22, x13
100443b74: aa0c03f5    	mov	x21, x12
100443b78: f94027e8    	ldr	x8, [sp, #0x48]
100443b7c: d1000508    	sub	x8, x8, #0x1
100443b80: f90027e8    	str	x8, [sp, #0x48]
100443b84: 17ffffc3    	b	0x100443a90 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x8e8>
100443b88: 794233ea    	ldrh	w10, [sp, #0x118]
100443b8c: 34fff28a    	cbz	w10, 0x1004439dc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x834>
100443b90: 794237eb    	ldrh	w11, [sp, #0x11a]
100443b94: 34fff24b    	cbz	w11, 0x1004439dc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x834>
100443b98: bd4103f0    	ldr	s16, [sp, #0x100]
100443b9c: bd4107e7    	ldr	s7, [sp, #0x104]
100443ba0: bd410bed    	ldr	s13, [sp, #0x108]
100443ba4: bd410fe5    	ldr	s5, [sp, #0x10c]
100443ba8: bd4113e0    	ldr	s0, [sp, #0x110]
100443bac: bd4117e3    	ldr	s3, [sp, #0x114]
100443bb0: 9e230321    	ucvtf	s1, x25
100443bb4: f9403be9    	ldr	x9, [sp, #0x70]
100443bb8: 2d42312f    	ldp	s15, s12, [x9, #0x10]
100443bbc: 1e2109ee    	fmul	s14, s15, s1
100443bc0: 1e280991    	fmul	s17, s12, s8
100443bc4: 34000778    	cbz	w24, 0x100443cb0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xb08>
100443bc8: 37000748    	tbnz	w8, #0x0, 0x100443cb0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xb08>
100443bcc: aa1403e9    	mov	x9, x20
100443bd0: b94037e8    	ldr	w8, [sp, #0x34]
100443bd4: 6b08027f    	cmp	w19, w8
100443bd8: f9403ff4    	ldr	x20, [sp, #0x78]
100443bdc: 54000d41    	b.ne	0x100443d84 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xbdc>
100443be0: b94033e8    	ldr	w8, [sp, #0x30]
100443be4: 6b08013f    	cmp	w9, w8
100443be8: 54000ce1    	b.ne	0x100443d84 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xbdc>
100443bec: d10333a8    	sub	x8, x29, #0xcc
100443bf0: 3cc04101    	ldur	q1, [x8, #0x4]
100443bf4: 3c9003a1    	stur	q1, [x29, #-0x100]
100443bf8: 1e230141    	ucvtf	s1, w10
100443bfc: 1e2f29e2    	fadd	s2, s15, s15
100443c00: 1e227824    	fminnm	s4, s1, s2
100443c04: 1e2e2812    	fadd	s18, s0, s14
100443c08: f9403be8    	ldr	x8, [sp, #0x70]
100443c0c: 14000069    	b	0x100443db0 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xc08>
100443c10: 794233f4    	ldrh	w20, [sp, #0x118]
100443c14: 34fff974    	cbz	w20, 0x100443b40 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x998>
100443c18: 794237e8    	ldrh	w8, [sp, #0x11a]
100443c1c: 34fff928    	cbz	w8, 0x100443b40 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x998>
100443c20: b9000be8    	str	w8, [sp, #0x8]
100443c24: bd4103e6    	ldr	s6, [sp, #0x100]
100443c28: bd4107e5    	ldr	s5, [sp, #0x104]
100443c2c: bd410be4    	ldr	s4, [sp, #0x108]
100443c30: bd410fe3    	ldr	s3, [sp, #0x10c]
100443c34: bd4113ef    	ldr	s15, [sp, #0x110]
100443c38: bd4117ee    	ldr	s14, [sp, #0x114]
100443c3c: 9e230320    	ucvtf	s0, x25
100443c40: f9403be8    	ldr	x8, [sp, #0x70]
100443c44: 2d420901    	ldp	s1, s2, [x8, #0x10]
100443c48: 1e20082d    	fmul	s13, s1, s0
100443c4c: 1e280840    	fmul	s0, s2, s8
100443c50: 2d030fe4    	stp	s4, s3, [sp, #0x18]
100443c54: 2d0217e6    	stp	s6, s5, [sp, #0x10]
100443c58: bd000fe0    	str	s0, [sp, #0xc]
100443c5c: 3400107b    	cbz	w27, 0x100443e68 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xcc0>
100443c60: 37001055    	tbnz	w21, #0x0, 0x100443e68 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xcc0>
100443c64: aa1603e0    	mov	x0, x22
100443c68: aa1b03e1    	mov	x1, x27
100443c6c: 9400023f    	bl	0x100444568 <__ZN11loom_render8terminal5glyph21color_glyph_cell_span17hdf9237eb60349ab5E>
100443c70: b94037e8    	ldr	w8, [sp, #0x34]
100443c74: 6b08035f    	cmp	w26, w8
100443c78: 54001641    	b.ne	0x100443f40 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xd98>
100443c7c: b94033e8    	ldr	w8, [sp, #0x30]
100443c80: 6b08027f    	cmp	w19, w8
100443c84: 540015e1    	b.ne	0x100443f40 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xd98>
100443c88: d10333a8    	sub	x8, x29, #0xcc
100443c8c: 3cc04100    	ldur	q0, [x8, #0x4]
100443c90: 3c9103a0    	stur	q0, [x29, #-0xf0]
100443c94: 1e230280    	ucvtf	s0, w20
100443c98: f9403be8    	ldr	x8, [sp, #0x70]
100443c9c: bd401101    	ldr	s1, [x8, #0x10]
100443ca0: 1e212821    	fadd	s1, s1, s1
100443ca4: 1e21780c    	fminnm	s12, s0, s1
100443ca8: 1e2d29ed    	fadd	s13, s15, s13
100443cac: 140000b1    	b	0x100443f70 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xdc8>
100443cb0: f9403ff4    	ldr	x20, [sp, #0x78]
100443cb4: 36000688    	tbz	w8, #0x0, 0x100443d84 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xbdc>
100443cb8: d10333a8    	sub	x8, x29, #0xcc
100443cbc: 3cc04100    	ldur	q0, [x8, #0x4]
100443cc0: 3c9003a0    	stur	q0, [x29, #-0x100]
100443cc4: f1000abf    	cmp	x21, #0x2
100443cc8: 52800048    	mov	w8, #0x2                ; =2
100443ccc: 9a8882b8    	csel	x24, x21, x8, hi
100443cd0: f9402ff6    	ldr	x22, [sp, #0x58]
100443cd4: f94016d5    	ldr	x21, [x22, #0x28]
100443cd8: f9400ec8    	ldr	x8, [x22, #0x18]
100443cdc: eb0802bf    	cmp	x21, x8
100443ce0: 54000161    	b.ne	0x100443d0c <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xb64>
100443ce4: 910062c0    	add	x0, x22, #0x18
100443ce8: 2d0397e7    	stp	s7, s5, [sp, #0x1c]
100443cec: 2d02c7f0    	stp	s16, s17, [sp, #0x14]
100443cf0: b90013ea    	str	w10, [sp, #0x10]
100443cf4: aa0b03f3    	mov	x19, x11
100443cf8: 94101138    	bl	0x1008481d8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hec22174b7db345eeE>
100443cfc: aa1303eb    	mov	x11, x19
100443d00: b94013ea    	ldr	w10, [sp, #0x10]
100443d04: 2d42c7f0    	ldp	s16, s17, [sp, #0x14]
100443d08: 2d4397e7    	ldp	s7, s5, [sp, #0x1c]
100443d0c: 1e230140    	ucvtf	s0, w10
100443d10: 1e230161    	ucvtf	s1, w11
100443d14: 9e230302    	ucvtf	s2, x24
100443d18: 1e2209e2    	fmul	s2, s15, s2
100443d1c: 1e201843    	fdiv	s3, s2, s0
100443d20: 1e211984    	fdiv	s4, s12, s1
100443d24: 1e247863    	fminnm	s3, s3, s4
100443d28: 1e200860    	fmul	s0, s3, s0
100443d2c: 1e210861    	fmul	s1, s3, s1
100443d30: 1e203842    	fsub	s2, s2, s0
100443d34: 1e2b0842    	fmul	s2, s2, s11
100443d38: 1e213983    	fsub	s3, s12, s1
100443d3c: 1e2b0863    	fmul	s3, s3, s11
100443d40: 1e2229c2    	fadd	s2, s14, s2
100443d44: 1e264042    	frinta	s2, s2
100443d48: f94012c8    	ldr	x8, [x22, #0x20]
100443d4c: 52800609    	mov	w9, #0x30               ; =48
100443d50: 9b0922a8    	madd	x8, x21, x9, x8
100443d54: d10333a9    	sub	x9, x29, #0xcc
100443d58: 3cc04124    	ldur	q4, [x9, #0x4]
100443d5c: 3d800104    	str	q4, [x8]
100443d60: 1e232a23    	fadd	s3, s17, s3
100443d64: 1e264063    	frinta	s3, s3
100443d68: 2d020d02    	stp	s2, s3, [x8, #0x10]
100443d6c: 2d030500    	stp	s0, s1, [x8, #0x18]
100443d70: 2d041d10    	stp	s16, s7, [x8, #0x20]
100443d74: 2d05150d    	stp	s13, s5, [x8, #0x28]
100443d78: 910006a8    	add	x8, x21, #0x1
100443d7c: f90016c8    	str	x8, [x22, #0x28]
100443d80: 14000030    	b	0x100443e40 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xc98>
100443d84: d10333a8    	sub	x8, x29, #0xcc
100443d88: 3cc04101    	ldur	q1, [x8, #0x4]
100443d8c: 3c9003a1    	stur	q1, [x29, #-0x100]
100443d90: f9403be8    	ldr	x8, [sp, #0x70]
100443d94: bd401d01    	ldr	s1, [x8, #0x1c]
100443d98: 1e2139e1    	fsub	s1, s15, s1
100443d9c: 1e2b0821    	fmul	s1, s1, s11
100443da0: 1e264021    	frinta	s1, s1
100443da4: 1e2e2800    	fadd	s0, s0, s14
100443da8: 1e212812    	fadd	s18, s0, s1
100443dac: 1e230144    	ucvtf	s4, w10
100443db0: bd40190f    	ldr	s15, [x8, #0x18]
100443db4: f9402ff6    	ldr	x22, [sp, #0x58]
100443db8: f9400ad5    	ldr	x21, [x22, #0x10]
100443dbc: f94002c8    	ldr	x8, [x22]
100443dc0: eb0802bf    	cmp	x21, x8
100443dc4: 54000221    	b.ne	0x100443e08 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xc60>
100443dc8: aa1603e0    	mov	x0, x22
100443dcc: 2d0397e7    	stp	s7, s5, [sp, #0x1c]
100443dd0: 1e6041ac    	fmov	d12, d13
100443dd4: 1e60420d    	fmov	d13, d16
100443dd8: 2d02c7e4    	stp	s4, s17, [sp, #0x14]
100443ddc: 1e60406e    	fmov	d14, d3
100443de0: aa0b03f3    	mov	x19, x11
100443de4: bd0013f2    	str	s18, [sp, #0x10]
100443de8: 941010fc    	bl	0x1008481d8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hec22174b7db345eeE>
100443dec: 2d4213f2    	ldp	s18, s4, [sp, #0x10]
100443df0: aa1303eb    	mov	x11, x19
100443df4: 1e6041c3    	fmov	d3, d14
100443df8: 2d431ff1    	ldp	s17, s7, [sp, #0x18]
100443dfc: 1e6041b0    	fmov	d16, d13
100443e00: 1e60418d    	fmov	d13, d12
100443e04: bd4023e5    	ldr	s5, [sp, #0x20]
100443e08: 1e230160    	ucvtf	s0, w11
100443e0c: 1e2f2a21    	fadd	s1, s17, s15
100443e10: 1e233821    	fsub	s1, s1, s3
100443e14: f94006c8    	ldr	x8, [x22, #0x8]
100443e18: 52800609    	mov	w9, #0x30               ; =48
100443e1c: 9b0922a8    	madd	x8, x21, x9, x8
100443e20: 3cd003a2    	ldur	q2, [x29, #-0x100]
100443e24: 3d800102    	str	q2, [x8]
100443e28: 2d020512    	stp	s18, s1, [x8, #0x10]
100443e2c: 2d030104    	stp	s4, s0, [x8, #0x18]
100443e30: 2d041d10    	stp	s16, s7, [x8, #0x20]
100443e34: 2d05150d    	stp	s13, s5, [x8, #0x28]
100443e38: 910006a8    	add	x8, x21, #0x1
100443e3c: f9000ac8    	str	x8, [x22, #0x10]
100443e40: f9404fe8    	ldr	x8, [sp, #0x98]
100443e44: aa1c03ec    	mov	x12, x28
100443e48: aa1703ed    	mov	x13, x23
100443e4c: 29453bf0    	ldp	w16, w14, [sp, #0x28]
100443e50: aa1a03ef    	mov	x15, x26
100443e54: aa1b03f1    	mov	x17, x27
100443e58: b94027e1    	ldr	w1, [sp, #0x24]
100443e5c: 91000508    	add	x8, x8, #0x1
100443e60: f9004fe8    	str	x8, [sp, #0x98]
100443e64: 17ffff1a    	b	0x100443acc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x924>
100443e68: aa1603e0    	mov	x0, x22
100443e6c: aa1b03e1    	mov	x1, x27
100443e70: 940001be    	bl	0x100444568 <__ZN11loom_render8terminal5glyph21color_glyph_cell_span17hdf9237eb60349ab5E>
100443e74: 36000675    	tbz	w21, #0x0, 0x100443f40 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xd98>
100443e78: b90007f4    	str	w20, [sp, #0x4]
100443e7c: f100081f    	cmp	x0, #0x2
100443e80: 52800048    	mov	w8, #0x2                ; =2
100443e84: 9a888013    	csel	x19, x0, x8, hi
100443e88: d10333a8    	sub	x8, x29, #0xcc
100443e8c: 3cc04100    	ldur	q0, [x8, #0x4]
100443e90: 3c9103a0    	stur	q0, [x29, #-0xf0]
100443e94: a94753e8    	ldp	x8, x20, [sp, #0x70]
100443e98: 2d42310e    	ldp	s14, s12, [x8, #0x10]
100443e9c: f9402ff6    	ldr	x22, [sp, #0x58]
100443ea0: f94016d5    	ldr	x21, [x22, #0x28]
100443ea4: f9400ec8    	ldr	x8, [x22, #0x18]
100443ea8: eb0802bf    	cmp	x21, x8
100443eac: 54000061    	b.ne	0x100443eb8 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xd10>
100443eb0: 910062c0    	add	x0, x22, #0x18
100443eb4: 941010c9    	bl	0x1008481d8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hec22174b7db345eeE>
100443eb8: 2940a3e9    	ldp	w9, w8, [sp, #0x4]
100443ebc: 1e230120    	ucvtf	s0, w9
100443ec0: 1e230101    	ucvtf	s1, w8
100443ec4: 9e230262    	ucvtf	s2, x19
100443ec8: 1e2209c2    	fmul	s2, s14, s2
100443ecc: 1e201843    	fdiv	s3, s2, s0
100443ed0: 1e211984    	fdiv	s4, s12, s1
100443ed4: 1e247863    	fminnm	s3, s3, s4
100443ed8: 1e200860    	fmul	s0, s3, s0
100443edc: 1e210861    	fmul	s1, s3, s1
100443ee0: 1e203842    	fsub	s2, s2, s0
100443ee4: 1e2b0842    	fmul	s2, s2, s11
100443ee8: 1e213983    	fsub	s3, s12, s1
100443eec: 1e2b0863    	fmul	s3, s3, s11
100443ef0: 1e2229a2    	fadd	s2, s13, s2
100443ef4: 1e264042    	frinta	s2, s2
100443ef8: f94012c8    	ldr	x8, [x22, #0x20]
100443efc: 52800609    	mov	w9, #0x30               ; =48
100443f00: 9b0922a8    	madd	x8, x21, x9, x8
100443f04: d10333a9    	sub	x9, x29, #0xcc
100443f08: 3cc04124    	ldur	q4, [x9, #0x4]
100443f0c: 3d800104    	str	q4, [x8]
100443f10: bd400fe4    	ldr	s4, [sp, #0xc]
100443f14: 1e232883    	fadd	s3, s4, s3
100443f18: 1e264063    	frinta	s3, s3
100443f1c: 2d020d02    	stp	s2, s3, [x8, #0x10]
100443f20: 2d030500    	stp	s0, s1, [x8, #0x18]
100443f24: 2d4203e1    	ldp	s1, s0, [sp, #0x10]
100443f28: 2d040101    	stp	s1, s0, [x8, #0x20]
100443f2c: 2d4303e1    	ldp	s1, s0, [sp, #0x18]
100443f30: 2d050101    	stp	s1, s0, [x8, #0x28]
100443f34: 910006a8    	add	x8, x21, #0x1
100443f38: f90016c8    	str	x8, [x22, #0x28]
100443f3c: 17ffff02    	b	0x100443b44 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x99c>
100443f40: d10333a8    	sub	x8, x29, #0xcc
100443f44: 3cc04100    	ldur	q0, [x8, #0x4]
100443f48: 3c9103a0    	stur	q0, [x29, #-0xf0]
100443f4c: f9403be8    	ldr	x8, [sp, #0x70]
100443f50: bd401100    	ldr	s0, [x8, #0x10]
100443f54: bd401d01    	ldr	s1, [x8, #0x1c]
100443f58: 1e213800    	fsub	s0, s0, s1
100443f5c: 1e2b0800    	fmul	s0, s0, s11
100443f60: 1e264000    	frinta	s0, s0
100443f64: 1e2d29e1    	fadd	s1, s15, s13
100443f68: 1e20282d    	fadd	s13, s1, s0
100443f6c: 1e23028c    	ucvtf	s12, w20
100443f70: f9403ff4    	ldr	x20, [sp, #0x78]
100443f74: bd40190f    	ldr	s15, [x8, #0x18]
100443f78: f9402ff6    	ldr	x22, [sp, #0x58]
100443f7c: f9400ad5    	ldr	x21, [x22, #0x10]
100443f80: f94002c8    	ldr	x8, [x22]
100443f84: eb0802bf    	cmp	x21, x8
100443f88: 54000061    	b.ne	0x100443f94 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xdec>
100443f8c: aa1603e0    	mov	x0, x22
100443f90: 94101092    	bl	0x1008481d8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hec22174b7db345eeE>
100443f94: b9400be8    	ldr	w8, [sp, #0x8]
100443f98: 1e230100    	ucvtf	s0, w8
100443f9c: bd400fe1    	ldr	s1, [sp, #0xc]
100443fa0: 1e2f2821    	fadd	s1, s1, s15
100443fa4: 1e2e3821    	fsub	s1, s1, s14
100443fa8: f94006c8    	ldr	x8, [x22, #0x8]
100443fac: 52800609    	mov	w9, #0x30               ; =48
100443fb0: 9b0922a8    	madd	x8, x21, x9, x8
100443fb4: 3cd103a2    	ldur	q2, [x29, #-0xf0]
100443fb8: 3d800102    	str	q2, [x8]
100443fbc: 2d02050d    	stp	s13, s1, [x8, #0x10]
100443fc0: 2d03010c    	stp	s12, s0, [x8, #0x18]
100443fc4: 2d4203e1    	ldp	s1, s0, [sp, #0x10]
100443fc8: 2d040101    	stp	s1, s0, [x8, #0x20]
100443fcc: 2d4303e1    	ldp	s1, s0, [sp, #0x18]
100443fd0: 2d050101    	stp	s1, s0, [x8, #0x28]
100443fd4: 910006a8    	add	x8, x21, #0x1
100443fd8: f9000ac8    	str	x8, [x22, #0x10]
100443fdc: 17fffeda    	b	0x100443b44 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x99c>
100443fe0: f9401fe1    	ldr	x1, [sp, #0x38]
100443fe4: f9402ffb    	ldr	x27, [sp, #0x58]
100443fe8: b940a7e8    	ldr	w8, [sp, #0xa4]
100443fec: 36000368    	tbz	w8, #0x0, 0x100444058 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xeb0>
100443ff0: f94033e8    	ldr	x8, [sp, #0x60]
100443ff4: 9e230100    	ucvtf	s0, x8
100443ff8: cb080288    	sub	x8, x20, x8
100443ffc: 9e230101    	ucvtf	s1, x8
100444000: f9403be8    	ldr	x8, [sp, #0x70]
100444004: 2d422d02    	ldp	s2, s11, [x8, #0x10]
100444008: 1e20084e    	fmul	s14, s2, s0
10044400c: 1e21084d    	fmul	s13, s2, s1
100444010: 1e280968    	fmul	s8, s11, s8
100444014: f9402375    	ldr	x21, [x27, #0x40]
100444018: f9401b68    	ldr	x8, [x27, #0x30]
10044401c: eb0802bf    	cmp	x21, x8
100444020: 540000a1    	b.ne	0x100444034 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xe8c>
100444024: 9100c360    	add	x0, x27, #0x30
100444028: aa0103f6    	mov	x22, x1
10044402c: 94101051    	bl	0x100848170 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hca29980afef496daE>
100444030: aa1603e1    	mov	x1, x22
100444034: f9401f68    	ldr	x8, [x27, #0x38]
100444038: 8b151508    	add	x8, x8, x21, lsl #5
10044403c: 2d00210e    	stp	s14, s8, [x8]
100444040: 2d012d0d    	stp	s13, s11, [x8, #0x8]
100444044: 3dc02fe0    	ldr	q0, [sp, #0xb0]
100444048: 2d020109    	stp	s9, s0, [x8, #0x10]
10044404c: 2d03290c    	stp	s12, s10, [x8, #0x18]
100444050: 910006a8    	add	x8, x21, #0x1
100444054: f9002368    	str	x8, [x27, #0x40]
100444058: f94023e9    	ldr	x9, [sp, #0x40]
10044405c: b4000cc9    	cbz	x9, 0x1004441f4 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x104c>
100444060: f9401528    	ldr	x8, [x9, #0x28]
100444064: b4000c88    	cbz	x8, 0x1004441f4 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x104c>
100444068: f9401129    	ldr	x9, [x9, #0x20]
10044406c: 8b080908    	add	x8, x8, x8, lsl #2
100444070: d37df115    	lsl	x21, x8, #3
100444074: f9400836    	ldr	x22, [x1, #0x10]
100444078: f9402be8    	ldr	x8, [sp, #0x50]
10044407c: 9e230100    	ucvtf	s0, x8
100444080: bd00c7e0    	str	s0, [sp, #0xc4]
100444084: 91004137    	add	x23, x9, #0x10
100444088: 52800058    	mov	w24, #0x2               ; =2
10044408c: 52800619    	mov	w25, #0x30              ; =48
100444090: 1400000e    	b	0x1004440c8 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xf20>
100444094: f9400768    	ldr	x8, [x27, #0x8]
100444098: 3cd203a0    	ldur	q0, [x29, #-0xe0]
10044409c: 9b192348    	madd	x8, x26, x25, x8
1004440a0: 3d800100    	str	q0, [x8]
1004440a4: 2d022109    	stp	s9, s8, [x8, #0x10]
1004440a8: 2d03390f    	stp	s15, s14, [x8, #0x18]
1004440ac: 2d04310d    	stp	s13, s12, [x8, #0x20]
1004440b0: 2d05290b    	stp	s11, s10, [x8, #0x28]
1004440b4: 91000748    	add	x8, x26, #0x1
1004440b8: f9000b68    	str	x8, [x27, #0x10]
1004440bc: 9100a2f7    	add	x23, x23, #0x28
1004440c0: f100a2b5    	subs	x21, x21, #0x28
1004440c4: 54000980    	b.eq	0x1004441f4 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x104c>
1004440c8: f85f82fa    	ldur	x26, [x23, #-0x8]
1004440cc: b94002e2    	ldr	w2, [x23]
1004440d0: 297e26e8    	ldp	w8, w9, [x23, #-0x10]
1004440d4: 394012e4    	ldrb	w4, [x23, #0x4]
1004440d8: 3cc082e0    	ldur	q0, [x23, #0x8]
1004440dc: 3c9203a0    	stur	q0, [x29, #-0xe0]
1004440e0: f94067eb    	ldr	x11, [sp, #0xc8]
1004440e4: b941316a    	ldr	w10, [x11, #0x130]
1004440e8: b941356b    	ldr	w11, [x11, #0x134]
1004440ec: 6b0a011f    	cmp	w8, w10
1004440f0: 7a4b0120    	ccmp	w9, w11, #0x0, eq
1004440f4: 54000100    	b.eq	0x100444114 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xf6c>
1004440f8: f94067eb    	ldr	x11, [sp, #0xc8]
1004440fc: b941396a    	ldr	w10, [x11, #0x138]
100444100: b9413d6b    	ldr	w11, [x11, #0x13c]
100444104: 6b0b013f    	cmp	w9, w11
100444108: 7a4a0100    	ccmp	w8, w10, #0x0, eq
10044410c: 1a9f0303    	csel	w3, w24, wzr, eq
100444110: 14000002    	b	0x100444118 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xf70>
100444114: 52800023    	mov	w3, #0x1                ; =1
100444118: d10333a0    	sub	x0, x29, #0xcc
10044411c: f94067e1    	ldr	x1, [sp, #0xc8]
100444120: 52800005    	mov	w5, #0x0                ; =0
100444124: 940048ff    	bl	0x100456520 <__ZN11loom_render11glyph_cache10GlyphCache26ensure_glyph_id_with_class17hb07ca767ff79e2c9E>
100444128: 385503a8    	ldurb	w8, [x29, #-0xb0]
10044412c: 7100091f    	cmp	w8, #0x2
100444130: 54fffc60    	b.eq	0x1004440bc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xf14>
100444134: 7854c3a9    	ldurh	w9, [x29, #-0xb4]
100444138: 34fffc29    	cbz	w9, 0x1004440bc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xf14>
10044413c: 7854e3aa    	ldurh	w10, [x29, #-0xb2]
100444140: 34fffbea    	cbz	w10, 0x1004440bc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xf14>
100444144: 2d6883a1    	ldp	s1, s0, [x29, #-0xbc]
100444148: 2d67abab    	ldp	s11, s10, [x29, #-0xc4]
10044414c: 9e230342    	ucvtf	s2, x26
100444150: 2d4212c3    	ldp	s3, s4, [x22, #0x10]
100444154: 1e220862    	fmul	s2, s3, s2
100444158: 2d4316c6    	ldp	s6, s5, [x22, #0x18]
10044415c: 1e253863    	fsub	s3, s3, s5
100444160: 2d66b3ad    	ldp	s13, s12, [x29, #-0xcc]
100444164: bd40c7e5    	ldr	s5, [sp, #0xc4]
100444168: 1e250884    	fmul	s4, s4, s5
10044416c: 1e2c1005    	fmov	s5, #0.50000000
100444170: 1e250863    	fmul	s3, s3, s5
100444174: 1e264063    	frinta	s3, s3
100444178: 1e222821    	fadd	s1, s1, s2
10044417c: 1e232829    	fadd	s9, s1, s3
100444180: 1e262881    	fadd	s1, s4, s6
100444184: 1e203828    	fsub	s8, s1, s0
100444188: 1e23012f    	ucvtf	s15, w9
10044418c: 1e23014e    	ucvtf	s14, w10
100444190: 36000248    	tbz	w8, #0x0, 0x1004441d8 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x1030>
100444194: f940177a    	ldr	x26, [x27, #0x28]
100444198: f9400f68    	ldr	x8, [x27, #0x18]
10044419c: eb08035f    	cmp	x26, x8
1004441a0: 54000061    	b.ne	0x1004441ac <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0x1004>
1004441a4: 91006360    	add	x0, x27, #0x18
1004441a8: 9410100c    	bl	0x1008481d8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hec22174b7db345eeE>
1004441ac: f9401368    	ldr	x8, [x27, #0x20]
1004441b0: 3cd203a0    	ldur	q0, [x29, #-0xe0]
1004441b4: 9b192348    	madd	x8, x26, x25, x8
1004441b8: 3d800100    	str	q0, [x8]
1004441bc: 2d022109    	stp	s9, s8, [x8, #0x10]
1004441c0: 2d03390f    	stp	s15, s14, [x8, #0x18]
1004441c4: 2d04310d    	stp	s13, s12, [x8, #0x20]
1004441c8: 2d05290b    	stp	s11, s10, [x8, #0x28]
1004441cc: 91000748    	add	x8, x26, #0x1
1004441d0: f9001768    	str	x8, [x27, #0x28]
1004441d4: 17ffffba    	b	0x1004440bc <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xf14>
1004441d8: f9400b7a    	ldr	x26, [x27, #0x10]
1004441dc: f9400368    	ldr	x8, [x27]
1004441e0: eb08035f    	cmp	x26, x8
1004441e4: 54fff581    	b.ne	0x100444094 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xeec>
1004441e8: aa1b03e0    	mov	x0, x27
1004441ec: 94100ffb    	bl	0x1008481d8 <__ZN5alloc7raw_vec19RawVec$LT$T$C$A$GT$8grow_one17hec22174b7db345eeE>
1004441f0: 17ffffa9    	b	0x100444094 <__ZN11loom_render8terminal4view23rebuild_row_render_data17haf3875d0bdaed05fE+0xeec>
1004441f4: 910683ff    	add	sp, sp, #0x1a0
1004441f8: a9497bfd    	ldp	x29, x30, [sp, #0x90]
1004441fc: a9484ff4    	ldp	x20, x19, [sp, #0x80]
100444200: a94757f6    	ldp	x22, x21, [sp, #0x70]
100444204: a9465ff8    	ldp	x24, x23, [sp, #0x60]
100444208: a94567fa    	ldp	x26, x25, [sp, #0x50]
10044420c: a9446ffc    	ldp	x28, x27, [sp, #0x40]
100444210: 6d4323e9    	ldp	d9, d8, [sp, #0x30]
100444214: 6d422beb    	ldp	d11, d10, [sp, #0x20]
100444218: 6d4133ed    	ldp	d13, d12, [sp, #0x10]
10044421c: 6cca3bef    	ldp	d15, d14, [sp], #0xa0
100444220: d65f03c0    	ret
