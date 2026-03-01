* Cache control instructions (68040+)
	.cpu	68040
	.text
* CINV - Cache Invalidate
	cinvl	dc,(a0)
	cinvl	ic,(a1)
	cinvl	bc,(a2)
	cinvp	dc,(a0)
	cinvp	ic,(a1)
	cinvp	bc,(a2)
	cinva	dc
	cinva	ic
	cinva	bc
* CPUSH - Cache Push
	cpushl	dc,(a0)
	cpushl	ic,(a1)
	cpushl	bc,(a2)
	cpushp	dc,(a3)
	cpushp	ic,(a4)
	cpushp	bc,(a5)
	cpusha	dc
	cpusha	ic
	cpusha	bc
	.end
