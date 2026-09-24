<?php
            function sink(string|false $s): void {}
            function f(\SimpleXMLElement $x): void {
                sink($x->asXML());
            }
        