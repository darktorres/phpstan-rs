<?php
            interface Iface {}
            class ImplA implements Iface {}
            function f(Iface $i, ImplA $a): void {
                if ($i === $a) { echo 1; }
            }
        