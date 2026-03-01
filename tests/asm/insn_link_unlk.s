* LINK / UNLK instructions (68000 base + 68020 LINK.L)
	.text
	link	a6,#0
	link	a6,#-4
	link	a6,#-256
	link	a0,#-8
	link	a5,#-32768
	unlk	a6
	unlk	a0
	unlk	a5
	.end
