* MOVEQ optimization boundary cases
* move.l #imm,Dn → MOVEQ when imm fits in -128..127
	.text
* Should become MOVEQ
	move.l	#0,d0
	move.l	#1,d1
	move.l	#127,d2
	move.l	#-1,d3
	move.l	#-128,d4
* Should NOT become MOVEQ (out of range)
	move.l	#128,d5
	move.l	#-129,d6
	move.l	#256,d7
	move.l	#$FFFF,d0
	move.l	#$10000,d1
	.end
