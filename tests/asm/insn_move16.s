* MOVE16 instruction (68040+)
	.cpu	68040
	.text
* Post-increment to post-increment
	move16	(a0)+,(a1)+
* Absolute long forms
	move16	(a0)+,$FF0000
	move16	$FF0000,(a0)+
* Indirect to absolute
	move16	(a0),$FF0000
	move16	$FF0000,(a0)
	.end
