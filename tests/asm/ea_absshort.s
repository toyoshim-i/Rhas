* Absolute short and long addressing
	.text
* Absolute short ($0000-$7FFF, $FFFF8000-$FFFFFFFF)
	move.l	$1000.w,d0
	move.l	d0,$2000.w
	move.w	$100.w,d1
* Absolute long
	move.l	$12345678,d0
	move.l	d0,$ABCDEF00
* Forced long with .l suffix
	move.l	$1000.l,d0
* PC-relative
	lea	data(pc),a0
	bra.s	done
data:
	.dc.l	$DEADBEEF
done:
	nop
	.end
