* Optimization level -c0: all optimizations disabled
	.text
* Quick conversion should NOT happen with -c0
	add.l	#1,d0
	add.w	#8,d1
	sub.l	#3,d2
* MOVEQ conversion should NOT happen
	move.l	#0,d0
	move.l	#127,d1
* 0-displacement should NOT be suppressed
	move.l	0(a0),d0
* Branch should remain as-is
label:
	bra.w	label
	bsr.w	label
	beq.w	label
	.end
