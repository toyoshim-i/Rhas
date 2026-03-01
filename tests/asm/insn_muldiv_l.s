* 68020+ long multiply/divide
	.cpu	68020
	.text
* MULS.L / MULU.L (32x32->32)
	muls.l	d0,d1
	mulu.l	d2,d3
	muls.l	(a0),d4
	mulu.l	#100,d5
* MULS.L / MULU.L (32x32->64)
	muls.l	d0,d1:d2
	mulu.l	d3,d4:d5
* DIVS.L / DIVU.L (32/32->32r:32q)
	divs.l	d0,d1
	divu.l	d2,d3
	divs.l	(a0),d4
	divu.l	#10,d5
* DIVS.L / DIVU.L with remainder
	divsl.l	d0,d1:d2
	divul.l	d3,d4:d5
	.end
