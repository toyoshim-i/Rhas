* Optimization level -c2: v2 compatible mode
* With -c2: NONULDISP=1, NOBRACUT=1, extended opts OFF
	.text
* Quick conversion: active (unless -q)
	add.l	#1,d0
	add.w	#8,d1
	move.l	#0,d0
	move.l	#100,d1
* 0-displacement: NOT suppressed (NONULDISP=1)
	move.l	0(a0),d0
	move.l	0(a1),d1
* Branch cut: NOT active (NOBRACUT=1)
label:
	bra.w	label
	bsr.w	label
	.end
