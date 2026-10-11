<?php
            function sink(bool $b): void {}
            function f(\SimpleXMLElement $x): void {
                sink($x->asXML('/work/out.xml'));
            }
        