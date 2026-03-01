* Memory indirect addressing modes (68020+)
	.cpu	68020
	.text
* Memory indirect post-indexed: ([bd,An],Xn,od)
	move.l	([4,a0],d0.l*1,8),d1
	move.l	([8,a1],d2.w*2,0),d3
* Memory indirect pre-indexed: ([bd,An,Xn],od)
	move.l	([4,a0,d0.l*1],8),d1
	move.l	([8,a1,d2.w*4],0),d3
* PC-relative memory indirect
	move.l	([lab,pc],d0.l*1,0),d1
	move.l	([lab,pc,d0.l*2],0),d2
	bra.s	done
lab:
	.dc.l	0
done:
	nop
	.end
