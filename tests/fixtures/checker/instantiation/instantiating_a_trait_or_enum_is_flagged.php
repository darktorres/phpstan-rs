<?php
            trait NewT {}
            enum NewE {}
            function f(): void {
                $a = new NewT();
                $b = new NewE();
            }
        