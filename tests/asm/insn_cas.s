* CAS / CAS2 (68020+)
	.cpu	68020
	.text
* CAS
	cas.b	d0,d1,(a0)
	cas.w	d2,d3,(a1)
	cas.l	d4,d5,(a2)
* CAS2
	cas2.w	d0:d1,d2:d3,(a0):(a1)
	cas2.l	d4:d5,d6:d7,(a2):(a3)
	.end
