* 68020+ instructions: EXTB, PACK, UNPK, MOVEC, LINK.L, RTD
	.cpu	68020
	.text
* EXTB (68020+)
	extb.l	d0
	extb.l	d7
* PACK / UNPK (68020+)
	pack	d0,d1,#0
	pack	-(a0),-(a1),#$5030
	unpk	d2,d3,#$3030
	unpk	-(a2),-(a3),#$3030
* RTD
	rtd	#0
	rtd	#8
	rtd	#-4
* LINK.L (68020+)
	link.l	a6,#-65536
* MOVEC (privileged, 68010+)
	.cpu	68010
	movec	vbr,d0
	movec	d1,vbr
	.end
