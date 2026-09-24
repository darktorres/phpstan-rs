<?php
            function needsFloat(float $x): void {}
            function test(): void { needsFloat("not a number"); }
        