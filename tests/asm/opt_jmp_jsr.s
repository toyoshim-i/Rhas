* JMP/JSR optimization with -c4
	.text
* JSR to local → should optimize
	jsr	func
	jsr	func2
* JMP to local → should optimize
	jmp	done
func:
	nop
	rts
func2:
	nop
	nop
	rts
done:
	nop
	.end
