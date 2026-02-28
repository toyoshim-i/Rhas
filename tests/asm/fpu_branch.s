	.68040
	.fpid	3

	fbne.w	target_w
	nop
target_w:
	nop

	fbne.l	target_l
	nop
target_l:
	nop

	fdbne	d0,target_d
	nop
target_d:
	nop
