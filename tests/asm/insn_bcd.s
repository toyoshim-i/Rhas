* BCD instructions (ABCD/SBCD)
	.text
* Data register to data register
	abcd	d0,d1
	abcd	d3,d4
	sbcd	d0,d1
	sbcd	d5,d6
* Memory to memory (predecrement)
	abcd	-(a0),-(a1)
	abcd	-(a2),-(a3)
	sbcd	-(a0),-(a1)
	sbcd	-(a4),-(a5)
	.end
