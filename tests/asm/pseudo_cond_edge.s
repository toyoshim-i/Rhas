* Conditional assembly edge cases
	.text
* .ifdef with defined symbol
myval	.equ	42
	.ifdef	myval
	move.w	#1,d0
	.else
	move.w	#0,d0
	.endif
* .ifndef with undefined symbol
	.ifndef	nosuchsym
	move.w	#2,d1
	.endif
* .elseif chaining
x	.equ	3
	.if	x==1
	move.w	#10,d2
	.elseif	x==2
	move.w	#20,d2
	.elseif	x==3
	move.w	#30,d2
	.else
	move.w	#40,d2
	.endif
* Nested .if
	.if	1
	.if	0
	nop
	.else
	move.w	#50,d3
	.endif
	.endif
* False branch should not assemble
	.if	0
	.fail	should not reach
	.endif
	.end
