* CHK instruction
	.text
* CHK.W (68000)
	chk	d1,d0
	chk	(a0),d1
	chk	#100,d2
	chk.w	d3,d4
* CHK.L (68020+)
	.cpu	68020
	chk.l	d5,d6
	chk.l	(a1),d7
	chk.l	#$10000,d0
	.end
