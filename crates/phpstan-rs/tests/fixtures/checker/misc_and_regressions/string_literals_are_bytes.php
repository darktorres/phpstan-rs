<?php
function f(): void {
    if ("\xff" === "\u{ff}") {
        echo 1;
    }
    if ("\101" === "A") {
        echo 2;
    }
}
