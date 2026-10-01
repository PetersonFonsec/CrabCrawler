import { Pipe, PipeTransform } from '@angular/core';

/** Quebra um texto em palavras para animações palavra a palavra. */
@Pipe({ name: 'splitWords' })
export class SplitWordsPipe implements PipeTransform {
  transform(text: string): string[] {
    return text.trim().split(/\s+/);
  }
}
