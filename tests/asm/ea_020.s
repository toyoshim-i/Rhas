* 68020+ addressing modes
	.cpu	68020
	.text
* Indexed with scale
	move.l	(a0,d0.w*2),d1
	move.l	(a0,d1.l*4),d2
	move.l	(a0,d2.l*8),d3
* Brief format indexed displacement
	move.l	(100,a0,d0.w),d4
	move.l	(-10,a1,d1.l*2),d5
* PC-relative indexed
	move.l	(start,pc,d0.w),d6
start:
	nop
* Full format: base displacement only
	move.l	($1234,a0),d0
	move.l	($12345678,a0),d1
	.end
