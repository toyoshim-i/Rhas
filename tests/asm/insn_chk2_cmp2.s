* CHK2 / CMP2 instructions (68020+)
	.cpu	68020
	.text
* CMP2 - compare register against bounds
	cmp2.b	bounds_b,d0
	cmp2.w	bounds_w,d1
	cmp2.l	bounds_l,a0
* CHK2 - check register against bounds (trap if out)
	chk2.b	bounds_b,d2
	chk2.w	bounds_w,d3
	chk2.l	bounds_l,a1
* With address register indirect
	cmp2.w	(a2),d4
	chk2.l	(a3),d5
	bra.s	done
bounds_b:
	.dc.b	0,$FF
bounds_w:
	.dc.w	0,$7FFF
bounds_l:
	.dc.l	0,$7FFFFFFF
done:
	nop
	.end
