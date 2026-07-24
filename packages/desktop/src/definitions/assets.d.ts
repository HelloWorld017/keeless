declare global {
  module 'binary:*' {
    const url: string;
    export default url;
  }

  module '*?asset' {
    const url: string;
    export default url;
  }
}

export {};
